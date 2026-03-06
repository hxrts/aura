//! Keyboard input handling and navigation logic.
//!
//! Processes keyboard events to navigate screens, update selections, handle text
//! input, and dispatch commands across the UI model state machine.

use crate::clipboard::ClipboardPort;
use crate::model::{
    AccessDepth, AddDeviceWizardStep, ChannelRow, CreateChannelDetailsField,
    CreateChannelWizardStep, ModalState, ThresholdWizardStep, ToastState, UiModel, UiScreen,
};
use aura_app::ui::types::parse_chat_command;

const SETTINGS_ROWS: usize = 6;

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

    if let Some(modal) = model.modal {
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
        if modal_accepts_text(model, modal) {
            model.modal_buffer.push(ch);
        }
        return;
    }

    match ch {
        '?' => {
            model.modal = Some(ModalState::Help);
            model.modal_hint = format!("Help - {}", model.screen.help_label());
            return;
        }
        '1' => {
            model.set_screen(UiScreen::Neighborhood);
            return;
        }
        '2' => {
            model.set_screen(UiScreen::Chat);
            return;
        }
        '3' => {
            model.set_screen(UiScreen::Contacts);
            return;
        }
        '4' => {
            model.set_screen(UiScreen::Notifications);
            return;
        }
        '5' => {
            model.set_screen(UiScreen::Settings);
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
        UiScreen::Chat => handle_chat_char(model, ch),
        UiScreen::Contacts => handle_contacts_char(model, ch),
        UiScreen::Neighborhood => handle_neighborhood_char(model, ch),
        UiScreen::Settings => handle_settings_char(model, ch),
        UiScreen::Notifications => {}
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
            model.modal = Some(ModalState::SetChannelTopic);
            model.modal_buffer = model.selected_channel_topic().to_string();
            model.modal_hint = "Set Channel Topic".to_string();
        }
        'o' => {
            model.modal = Some(ModalState::ChannelInfo);
            let channel = model.selected_channel_name().unwrap_or("general");
            model.modal_hint = format!("Channel: #{channel}");
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
            model.modal = Some(ModalState::CreateInvitation);
            model.modal_buffer.clear();
            model.modal_hint = "Invite Contacts".to_string();
        }
        'a' => {
            model.modal = Some(ModalState::AcceptInvitation);
            model.modal_buffer.clear();
            model.modal_hint = "Accept Invitation".to_string();
        }
        'e' => {
            model.modal = Some(ModalState::EditNickname);
            model.modal_buffer = model
                .selected_contact_name()
                .unwrap_or_default()
                .to_string();
            model.modal_hint = "Edit Nickname".to_string();
        }
        'g' => {
            open_guardian_setup_wizard(model);
        }
        'c' => {
            if let Some(contact) = model.selected_contact_name().map(str::to_string) {
                model.set_screen(UiScreen::Chat);
                model.select_channel_by_name(&format!("DM: {contact}"));
            }
        }
        'd' => {
            model.last_scan = "just now".to_string();
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "No LAN peers yet. Press d to rescan.".to_string(),
            });
        }
        'p' => {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "No LAN peers yet. Press d to rescan.".to_string(),
            });
        }
        'r' => {
            model.modal = Some(ModalState::RemoveContact);
            model.modal_hint = "Remove Contact".to_string();
        }
        _ => {}
    }
}

fn handle_neighborhood_char(model: &mut UiModel, ch: char) {
    match ch {
        'n' => {
            model.modal = Some(ModalState::CreateHome);
            model.modal_buffer.clear();
            model.modal_hint = "Create New Home".to_string();
        }
        'a' => {
            model.modal = Some(ModalState::AcceptInvitation);
            model.modal_buffer.clear();
            model.modal_hint = "Accept Invitation".to_string();
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "home invitation".to_string(),
            });
        }
        'd' => {
            model.access_depth = model.access_depth.next();
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: model.access_depth.compact().to_string(),
            });
        }
        'm' => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "neighborhood updated".to_string(),
            });
        }
        'v' => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "home added to neighborhood".to_string(),
            });
        }
        'L' => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "link membership updated".to_string(),
            });
        }
        'g' | 'H' => {
            if model.selected_home.is_none() {
                model.selected_home = Some("Neighborhood".to_string());
            }
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "Neighborhood".to_string(),
            });
        }
        'b' => {
            model.access_depth = AccessDepth::Limited;
            model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "Neighborhood".to_string(),
            });
        }
        'o' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal = Some(ModalState::AssignModerator);
            model.modal_hint = "Assign Moderator".to_string();
        }
        'x' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal = Some(ModalState::AccessOverride);
            model.reset_access_override_editor();
            model.modal_hint = "Access Override".to_string();
        }
        'p' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal = Some(ModalState::CapabilityConfig);
            model.reset_capability_config_editor();
            model.modal_buffer = model.capability_full_caps.clone();
            model.modal_hint = "Home Capability Configuration".to_string();
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
        'e' if model.settings_index == 0 => {
            model.modal = Some(ModalState::EditNickname);
            model.modal_buffer = model.profile_nickname.clone();
            model.modal_hint = "Edit Nickname".to_string();
        }
        't' if model.settings_index == 1 => {
            if can_open_guardian_setup_wizard(model) {
                open_guardian_setup_wizard(model);
            }
        }
        'a' if model.settings_index == 3 => {
            open_add_device_wizard(model);
        }
        'i' if model.settings_index == 3 => {
            model.modal = Some(ModalState::ImportDeviceEnrollmentCode);
            model.modal_hint = "Import Device Enrollment Code".to_string();
        }
        'r' if model.settings_index == 3 => {
            if model.has_secondary_device {
                open_remove_device_selection(model);
            } else {
                set_toast(model, 'ℹ', "Cannot remove the current device");
            }
        }
        's' if model.settings_index == 2 => {
            model.modal = Some(ModalState::RequestRecovery);
            model.modal_hint = "Request Recovery".to_string();
        }
        's' if model.settings_index == 4 => {
            if model.authorities.len() <= 1 {
                set_toast(model, 'ℹ', "Only one authority available");
            } else {
                model.modal = Some(ModalState::SwitchAuthority);
                model.modal_hint = "Switch Authority".to_string();
            }
        }
        'm' if model.settings_index == 4 => {
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

    if let Some(modal) = model.modal {
        handle_modal_enter(model, modal, clipboard);
        return;
    }

    match model.screen {
        UiScreen::Neighborhood => {
            if matches!(model.neighborhood_mode, crate::model::NeighborhoodMode::Map) {
                model.neighborhood_mode = crate::model::NeighborhoodMode::Detail;
            } else {
                model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
            }
        }
        UiScreen::Contacts => {
            model.contact_details = true;
        }
        UiScreen::Settings => match model.settings_index {
            0 => {
                model.modal = Some(ModalState::EditNickname);
                model.modal_buffer = model.profile_nickname.clone();
                model.modal_hint = "Edit Nickname".to_string();
            }
            1 => {
                if can_open_guardian_setup_wizard(model) {
                    open_guardian_setup_wizard(model);
                }
            }
            2 => {
                model.modal = Some(ModalState::RequestRecovery);
                model.modal_hint = "Request Recovery".to_string();
            }
            3 => {
                open_add_device_wizard(model);
            }
            4 => {
                if can_open_mfa_setup_wizard(model) {
                    open_mfa_setup_wizard(model);
                }
            }
            _ => {}
        },
        UiScreen::Chat | UiScreen::Notifications => {}
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
            model.modal_hint = format!("Invitation Created {code}");
            model.toast = Some(ToastState {
                icon: '✓',
                message: format!("Invitation Created {code}"),
            });
        }
        ModalState::AcceptInvitation => {
            let value = model.modal_buffer.trim();
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
            let name = model.modal_buffer.trim().to_string();
            if !name.is_empty() {
                model.selected_home = Some(name.clone());
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Home '{name}' created"),
                });
            }
            dismiss_modal(model);
        }
        ModalState::CreateChannel => match model.create_channel_step {
            CreateChannelWizardStep::Details => {
                save_create_channel_details_buffer(model);
                let channel = model
                    .create_channel_name
                    .trim()
                    .trim_start_matches('#')
                    .to_string();
                if channel.is_empty() {
                    set_toast(model, '✗', "Channel name is required");
                    return;
                }
                model.create_channel_name = channel;
                model.create_channel_step = CreateChannelWizardStep::Members;
                model.create_channel_member_focus = 0;
                model.modal_buffer.clear();
                model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
            }
            CreateChannelWizardStep::Members => {
                let selected_count = model.create_channel_selected_members.len();
                let participants = selected_count.saturating_add(1);
                model.create_channel_threshold = participants.max(1) as u8;
                model.create_channel_step = CreateChannelWizardStep::Threshold;
                model.modal_buffer = model.create_channel_threshold.to_string();
                model.modal_hint = "New Chat Group — Step 3 of 3".to_string();
            }
            CreateChannelWizardStep::Threshold => {
                if let Ok(value) = model.modal_buffer.trim().parse::<u8>() {
                    let max_threshold = (model
                        .create_channel_selected_members
                        .len()
                        .saturating_add(1)) as u8;
                    model.create_channel_threshold = value.clamp(1, max_threshold.max(1));
                }
                let channel = model.create_channel_name.trim().to_string();
                model.select_channel_by_name(&channel);
                if !model.create_channel_topic.trim().is_empty() {
                    model.set_selected_channel_topic(model.create_channel_topic.clone());
                }
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Created '{channel}'."),
                });
                dismiss_modal(model);
            }
        },
        ModalState::SetChannelTopic => {
            model.set_selected_channel_topic(model.modal_buffer.trim().to_string());
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Topic updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::EditNickname => {
            let value = model.modal_buffer.trim().to_string();
            if model.screen == UiScreen::Settings {
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
                model.contacts.remove(model.selected_contact_index);
                model.selected_contact_index = model.selected_contact_index.saturating_sub(1);
                for (idx, contact) in model.contacts.iter_mut().enumerate() {
                    contact.selected = idx == model.selected_contact_index;
                }
            }
            model.toast = Some(ToastState {
                icon: '✓',
                message: "membership updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::GuardianSetup => match model.guardian_wizard_step {
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
                if model.guardian_selected_indices.len() < 2 {
                    set_toast(model, '✗', "Select at least 2 guardians");
                    return;
                }
                let selected = model.guardian_selected_indices.len() as u8;
                model.guardian_selected_count = selected;
                model.guardian_threshold_k = model.guardian_threshold_k.clamp(1, selected.max(1));
                model.guardian_wizard_step = ThresholdWizardStep::Threshold;
                model.modal_buffer = model.guardian_threshold_k.to_string();
                model.modal_hint = "Guardian Setup — Step 2 of 3".to_string();
            }
            ThresholdWizardStep::Threshold => {
                let k = parse_wizard_value(&model.modal_buffer, model.guardian_threshold_k)
                    .clamp(1, model.guardian_selected_count);
                model.guardian_threshold_k = k;
                model.guardian_wizard_step = ThresholdWizardStep::Ceremony;
                model.modal_buffer.clear();
                model.modal_hint = "Guardian Setup — Step 3 of 3".to_string();
            }
            ThresholdWizardStep::Ceremony => {
                set_toast(
                    model,
                    'ℹ',
                    format!(
                        "Guardian ceremony started! Waiting for {}-of-{} guardians to respond",
                        model.guardian_threshold_k, model.guardian_selected_count
                    ),
                );
                dismiss_modal(model);
            }
        },
        ModalState::RequestRecovery => {
            let required = model.guardian_threshold_k.max(1) as usize;
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
        ModalState::AddDeviceStep1 => match model.add_device_step {
            AddDeviceWizardStep::Name => {
                let name = model.modal_buffer.trim().to_string();
                if name.is_empty() {
                    set_toast(model, '✗', "Device name is required");
                    return;
                }

                model.add_device_name = name;
                model.device_enrollment_counter = model.device_enrollment_counter.saturating_add(1);
                model.add_device_enrollment_code =
                    format!("DEVICE-ENROLL-{}", model.device_enrollment_counter);
                model.add_device_step = AddDeviceWizardStep::ShareCode;
                model.modal_buffer.clear();
                model.modal_hint = "Add Device — Step 2 of 3".to_string();
            }
            AddDeviceWizardStep::ShareCode => {
                model.add_device_step = AddDeviceWizardStep::Confirm;
                model.modal_hint = "Add Device — Step 3 of 3".to_string();
            }
            AddDeviceWizardStep::Confirm => {
                set_toast(model, 'ℹ', "Device enrollment started");
                dismiss_modal(model);
            }
        },
        ModalState::ImportDeviceEnrollmentCode => {
            if model.modal_buffer.trim().is_empty() {
                set_toast(model, '✗', "Enrollment code is required");
                return;
            }
            model.has_secondary_device = true;
            if model.secondary_device_name().is_none() {
                let fallback = if model.add_device_name.trim().is_empty() {
                    "Mobile".to_string()
                } else {
                    model.add_device_name.clone()
                };
                model.set_secondary_device_name(Some(fallback));
            }
            set_toast(model, '✓', "Device enrollment complete");
            dismiss_modal(model);
        }
        ModalState::SelectDeviceToRemove => {
            model.modal = Some(ModalState::ConfirmRemoveDevice);
            model.modal_hint = "Confirm Device Removal".to_string();
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
        ModalState::MfaSetup => match model.mfa_wizard_step {
            ThresholdWizardStep::Selection => {
                if model.mfa_selected_indices.is_empty() {
                    set_toast(model, '✗', "Select at least 1 device");
                    return;
                }
                let selected = model.mfa_selected_indices.len() as u8;
                model.mfa_selected_count = selected;
                model.mfa_threshold_k = model.mfa_threshold_k.clamp(1, model.mfa_selected_count);
                model.mfa_wizard_step = ThresholdWizardStep::Threshold;
                model.modal_buffer = model.mfa_threshold_k.to_string();
                model.modal_hint = "Multifactor Setup — Step 2 of 3".to_string();
            }
            ThresholdWizardStep::Threshold => {
                let k = parse_wizard_value(&model.modal_buffer, model.mfa_threshold_k)
                    .clamp(1, model.mfa_selected_count);
                model.mfa_threshold_k = k;
                model.mfa_wizard_step = ThresholdWizardStep::Ceremony;
                model.modal_buffer.clear();
                model.modal_hint = "Multifactor Setup — Step 3 of 3".to_string();
            }
            ThresholdWizardStep::Ceremony => {
                set_toast(
                    model,
                    'ℹ',
                    format!(
                        "Multifactor ceremony started ({}-of-{})",
                        model.mfa_threshold_k, model.mfa_selected_count
                    ),
                );
                dismiss_modal(model);
            }
        },
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
            save_capability_config_buffer(model);
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
                        model.select_channel_by_name(&channel);
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
                                    name: "general".to_string(),
                                    selected: true,
                                    topic: String::new(),
                                });
                                model.selected_channel_index = 0;
                            } else {
                                model.selected_channel_index = 0;
                                for (idx, row) in model.channels.iter_mut().enumerate() {
                                    row.selected = idx == 0;
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
                        ensure_dm_channel(model);
                        model.select_channel_by_name("dm");
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
                            "home invitation",
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
    if let Some(modal) = model.modal {
        if handle_modal_escape(model, modal) {
            return;
        }
        dismiss_modal(model);
        return;
    }
    if model.screen == UiScreen::Contacts && model.contact_details {
        model.contact_details = false;
        return;
    }
    if model.screen == UiScreen::Neighborhood
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

fn ensure_dm_channel(model: &mut UiModel) {
    if model
        .channels
        .iter()
        .any(|row| row.name.eq_ignore_ascii_case("dm"))
    {
        return;
    }
    model.channels.push(ChannelRow {
        name: "dm".to_string(),
        selected: false,
        topic: String::new(),
    });
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
    model.modal = None;
    model.modal_buffer.clear();
    model.modal_hint.clear();
    model.reset_create_channel_wizard();
    model.reset_add_device_wizard();
    model.reset_guardian_wizard();
    model.reset_mfa_wizard();
    model.reset_remove_device_flow();
    model.reset_capability_config_editor();
    model.reset_access_override_editor();
}

fn handle_modal_tab(model: &mut UiModel, reverse: bool) -> bool {
    if matches!(model.modal, Some(ModalState::CreateChannel))
        && matches!(model.create_channel_step, CreateChannelWizardStep::Details)
    {
        save_create_channel_details_buffer(model);
        if reverse {
            model.create_channel_active_field = CreateChannelDetailsField::Name;
        } else {
            model.create_channel_active_field = match model.create_channel_active_field {
                CreateChannelDetailsField::Name => CreateChannelDetailsField::Topic,
                CreateChannelDetailsField::Topic => CreateChannelDetailsField::Name,
            };
        }
        model.modal_buffer = match model.create_channel_active_field {
            CreateChannelDetailsField::Name => model.create_channel_name.clone(),
            CreateChannelDetailsField::Topic => model.create_channel_topic.clone(),
        };
        return true;
    }

    if matches!(model.modal, Some(ModalState::CapabilityConfig)) {
        save_capability_config_buffer(model);
        if reverse {
            model.capability_active_field = model.capability_active_field.saturating_sub(1);
        } else {
            model.capability_active_field = (model.capability_active_field + 1) % 3;
        }
        model.modal_buffer = match model.capability_active_field {
            0 => model.capability_full_caps.clone(),
            1 => model.capability_partial_caps.clone(),
            _ => model.capability_limited_caps.clone(),
        };
        return true;
    }

    if matches!(model.modal, Some(ModalState::AccessOverride)) {
        model.access_override_partial = !model.access_override_partial;
        return true;
    }

    false
}

fn modal_accepts_text(model: &UiModel, modal: ModalState) -> bool {
    if matches!(modal, ModalState::AddDeviceStep1) {
        return matches!(model.add_device_step, AddDeviceWizardStep::Name);
    }
    if matches!(modal, ModalState::GuardianSetup) {
        return matches!(model.guardian_wizard_step, ThresholdWizardStep::Threshold);
    }
    if matches!(modal, ModalState::MfaSetup) {
        return matches!(model.mfa_wizard_step, ThresholdWizardStep::Threshold);
    }
    if matches!(modal, ModalState::CreateChannel) {
        return matches!(
            model.create_channel_step,
            CreateChannelWizardStep::Details | CreateChannelWizardStep::Threshold
        );
    }
    if matches!(modal, ModalState::CapabilityConfig) {
        return true;
    }
    matches!(
        modal,
        ModalState::CreateInvitation
            | ModalState::AcceptInvitation
            | ModalState::CreateHome
            | ModalState::SetChannelTopic
            | ModalState::EditNickname
            | ModalState::ImportDeviceEnrollmentCode
    )
}

fn backspace(model: &mut UiModel) {
    if model.input_mode {
        model.input_buffer.pop();
    } else if let Some(modal) = model.modal {
        if modal_accepts_text(model, modal) {
            model.modal_buffer.pop();
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
        ModalState::CreateChannel => match model.create_channel_step {
            CreateChannelWizardStep::Members => {
                if matches!(ch, ' ') {
                    toggle_create_channel_member(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if ch == 'k' {
                        model.create_channel_member_focus =
                            model.create_channel_member_focus.saturating_sub(1);
                    } else {
                        model.create_channel_member_focus =
                            (model.create_channel_member_focus + 1).min(max);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        },
        ModalState::GuardianSetup => {
            if matches!(model.guardian_wizard_step, ThresholdWizardStep::Selection) {
                if matches!(ch, ' ') {
                    toggle_guardian_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if ch == 'k' {
                        model.guardian_focus_index = model.guardian_focus_index.saturating_sub(1);
                    } else {
                        model.guardian_focus_index = (model.guardian_focus_index + 1).min(max);
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
            if matches!(model.mfa_wizard_step, ThresholdWizardStep::Selection) {
                if matches!(ch, ' ') {
                    toggle_mfa_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    let max = available_device_count(model).saturating_sub(1) as usize;
                    if ch == 'k' {
                        model.mfa_focus_index = model.mfa_focus_index.saturating_sub(1);
                    } else {
                        model.mfa_focus_index = (model.mfa_focus_index + 1).min(max);
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
        ModalState::CreateChannel => match model.create_channel_step {
            CreateChannelWizardStep::Details => false,
            CreateChannelWizardStep::Members => {
                model.create_channel_step = CreateChannelWizardStep::Details;
                model.create_channel_active_field = CreateChannelDetailsField::Name;
                model.modal_buffer = model.create_channel_name.clone();
                model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
                true
            }
            CreateChannelWizardStep::Threshold => {
                model.create_channel_step = CreateChannelWizardStep::Members;
                model.modal_buffer.clear();
                model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
                true
            }
        },
        ModalState::GuardianSetup => match model.guardian_wizard_step {
            ThresholdWizardStep::Selection => false,
            ThresholdWizardStep::Threshold => {
                model.guardian_wizard_step = ThresholdWizardStep::Selection;
                model.modal_buffer.clear();
                model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
                true
            }
            ThresholdWizardStep::Ceremony => false,
        },
        ModalState::MfaSetup => match model.mfa_wizard_step {
            ThresholdWizardStep::Selection => false,
            ThresholdWizardStep::Threshold => {
                model.mfa_wizard_step = ThresholdWizardStep::Selection;
                model.modal_buffer.clear();
                model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
                true
            }
            ThresholdWizardStep::Ceremony => false,
        },
        _ => false,
    }
}

fn open_create_channel_wizard(model: &mut UiModel) {
    model.modal = Some(ModalState::CreateChannel);
    model.reset_create_channel_wizard();
    model.modal_buffer = model.create_channel_name.clone();
    model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
}

fn save_create_channel_details_buffer(model: &mut UiModel) {
    match model.create_channel_active_field {
        CreateChannelDetailsField::Name => {
            model.create_channel_name = model.modal_buffer.clone();
        }
        CreateChannelDetailsField::Topic => {
            model.create_channel_topic = model.modal_buffer.clone();
        }
    }
}

fn toggle_create_channel_member(model: &mut UiModel) {
    if model.contacts.is_empty() {
        return;
    }
    let idx = model
        .create_channel_member_focus
        .min(model.contacts.len().saturating_sub(1));
    if let Some(position) = model
        .create_channel_selected_members
        .iter()
        .position(|selected| *selected == idx)
    {
        model.create_channel_selected_members.remove(position);
    } else {
        model.create_channel_selected_members.push(idx);
        model.create_channel_selected_members.sort_unstable();
    }
}

fn open_add_device_wizard(model: &mut UiModel) {
    model.modal = Some(ModalState::AddDeviceStep1);
    model.reset_add_device_wizard();
    model.modal_buffer.clear();
    model.modal_hint = "Add Device — Step 1 of 3".to_string();
}

fn open_guardian_setup_wizard(model: &mut UiModel) {
    model.modal = Some(ModalState::GuardianSetup);
    model.reset_guardian_wizard();
    model.guardian_selected_indices = model
        .contacts
        .iter()
        .enumerate()
        .filter(|(_, contact)| contact.is_guardian)
        .map(|(idx, _)| idx)
        .collect();
    if model.guardian_selected_indices.is_empty() {
        let selected = model.contacts.len().min(2);
        model.guardian_selected_indices = (0..selected).collect();
    }
    let selected = model.guardian_selected_indices.len();
    model.guardian_selected_count = selected.max(1) as u8;
    model.guardian_threshold_k = model.guardian_selected_count.clamp(1, 2);
    model.modal_buffer.clear();
    model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
}

fn can_open_guardian_setup_wizard(model: &mut UiModel) -> bool {
    if model.contacts.is_empty() {
        set_toast(model, '✗', "Add contacts first before setting up guardians");
        return false;
    }
    true
}

fn open_remove_device_selection(model: &mut UiModel) {
    model.modal = Some(ModalState::SelectDeviceToRemove);
    model.remove_device_candidate_name = model
        .secondary_device_name()
        .unwrap_or("Secondary device")
        .to_string();
    model.modal_hint = "Select Device to Remove".to_string();
}

fn open_mfa_setup_wizard(model: &mut UiModel) {
    model.modal = Some(ModalState::MfaSetup);
    model.reset_mfa_wizard();
    model.mfa_selected_indices = (0..available_device_count(model) as usize).collect();
    model.mfa_selected_count = model.mfa_selected_indices.len().max(1) as u8;
    model.mfa_threshold_k = model.mfa_selected_count.min(2).max(1);
    model.modal_buffer.clear();
    model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
}

fn save_capability_config_buffer(model: &mut UiModel) {
    match model.capability_active_field {
        0 => model.capability_full_caps = model.modal_buffer.clone(),
        1 => model.capability_partial_caps = model.modal_buffer.clone(),
        _ => model.capability_limited_caps = model.modal_buffer.clone(),
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
    match model.add_device_step {
        AddDeviceWizardStep::Name => {
            model.modal_buffer.push(ch);
        }
        AddDeviceWizardStep::ShareCode | AddDeviceWizardStep::Confirm => {
            if matches!(ch, 'c' | 'y') && !model.add_device_enrollment_code.is_empty() {
                clipboard.write(&model.add_device_enrollment_code);
                model.add_device_code_copied = true;
                set_toast(model, '✓', "Copied to clipboard");
            }
        }
    }
}

fn handle_wizard_named_key(model: &mut UiModel, key_name: &str) -> bool {
    match model.modal {
        Some(ModalState::CreateChannel) => {
            if matches!(key_name, "up" | "down") {
                match model.create_channel_step {
                    CreateChannelWizardStep::Members => {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        if key_name == "up" {
                            model.create_channel_member_focus =
                                model.create_channel_member_focus.saturating_sub(1);
                        } else {
                            model.create_channel_member_focus =
                                (model.create_channel_member_focus + 1).min(max);
                        }
                        return true;
                    }
                    CreateChannelWizardStep::Threshold => {
                        let current =
                            parse_wizard_value(&model.modal_buffer, model.create_channel_threshold);
                        let max = (model
                            .create_channel_selected_members
                            .len()
                            .saturating_add(1)) as u8;
                        let adjusted = if key_name == "up" {
                            current.saturating_add(1).min(max.max(1))
                        } else {
                            current.saturating_sub(1).max(1)
                        };
                        model.modal_buffer = adjusted.to_string();
                        return true;
                    }
                    CreateChannelWizardStep::Details => {}
                }
            }
        }
        Some(ModalState::GuardianSetup) => {
            if matches!(key_name, "up" | "down") {
                match model.guardian_wizard_step {
                    ThresholdWizardStep::Selection => {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        if key_name == "up" {
                            model.guardian_focus_index =
                                model.guardian_focus_index.saturating_sub(1);
                        } else {
                            model.guardian_focus_index = (model.guardian_focus_index + 1).min(max);
                        }
                    }
                    ThresholdWizardStep::Threshold => {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, true, delta);
                    }
                    ThresholdWizardStep::Ceremony => {}
                }
                return true;
            }
        }
        Some(ModalState::MfaSetup) => {
            if matches!(key_name, "up" | "down") {
                match model.mfa_wizard_step {
                    ThresholdWizardStep::Selection => {
                        let max = available_device_count(model).saturating_sub(1) as usize;
                        if key_name == "up" {
                            model.mfa_focus_index = model.mfa_focus_index.saturating_sub(1);
                        } else {
                            model.mfa_focus_index = (model.mfa_focus_index + 1).min(max);
                        }
                    }
                    ThresholdWizardStep::Threshold => {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, false, delta);
                    }
                    ThresholdWizardStep::Ceremony => {}
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
                if key_name == "up" {
                    model.set_selected_authority_index(
                        model.selected_authority_index.saturating_sub(1),
                    );
                } else {
                    model.set_selected_authority_index(
                        (model.selected_authority_index + 1).min(max),
                    );
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
                if key_name == "up" {
                    model
                        .set_selected_contact_index(model.selected_contact_index.saturating_sub(1));
                } else {
                    model.set_selected_contact_index((model.selected_contact_index + 1).min(max));
                }
                return true;
            }
        }
        _ => {}
    }
    false
}

fn adjust_threshold_wizard_input(model: &mut UiModel, guardian: bool, delta: i8) {
    let (current, max) = if guardian {
        (
            parse_wizard_value(&model.modal_buffer, model.guardian_threshold_k),
            model.guardian_selected_count.max(1),
        )
    } else {
        (
            parse_wizard_value(&model.modal_buffer, model.mfa_threshold_k),
            model.mfa_selected_count.max(1),
        )
    };

    let adjusted = (current as i16 + delta as i16).clamp(1, max as i16) as u8;
    model.modal_buffer = adjusted.to_string();
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
    let idx = model
        .guardian_focus_index
        .min(model.contacts.len().saturating_sub(1));
    if let Some(position) = model
        .guardian_selected_indices
        .iter()
        .position(|selected| *selected == idx)
    {
        model.guardian_selected_indices.remove(position);
    } else {
        model.guardian_selected_indices.push(idx);
        model.guardian_selected_indices.sort_unstable();
    }
}

fn toggle_mfa_selection(model: &mut UiModel) {
    let available = available_device_count(model) as usize;
    if available == 0 {
        return;
    }
    let idx = model.mfa_focus_index.min(available.saturating_sub(1));
    if let Some(position) = model
        .mfa_selected_indices
        .iter()
        .position(|selected| *selected == idx)
    {
        model.mfa_selected_indices.remove(position);
    } else {
        model.mfa_selected_indices.push(idx);
        model.mfa_selected_indices.sort_unstable();
    }
}

fn cycle_screen(model: &mut UiModel) {
    let next = match model.screen {
        UiScreen::Neighborhood => UiScreen::Chat,
        UiScreen::Chat => UiScreen::Contacts,
        UiScreen::Contacts => UiScreen::Notifications,
        UiScreen::Notifications => UiScreen::Settings,
        UiScreen::Settings => UiScreen::Neighborhood,
    };
    model.set_screen(next);
}

fn cycle_screen_prev(model: &mut UiModel) {
    let next = match model.screen {
        UiScreen::Neighborhood => UiScreen::Settings,
        UiScreen::Chat => UiScreen::Neighborhood,
        UiScreen::Contacts => UiScreen::Chat,
        UiScreen::Notifications => UiScreen::Contacts,
        UiScreen::Settings => UiScreen::Notifications,
    };
    model.set_screen(next);
}

fn move_selection(model: &mut UiModel, delta: i32) {
    match model.screen {
        UiScreen::Settings => {
            let current = model.settings_index as i32;
            let mut next = current + delta;
            if next < 0 {
                next = 0;
            }
            if next >= SETTINGS_ROWS as i32 {
                next = SETTINGS_ROWS as i32 - 1;
            }
            model.settings_index = next as usize;
        }
        UiScreen::Contacts => {
            if model.contacts.is_empty() {
                return;
            }
            let max = model.contacts.len() as i32 - 1;
            let mut next = model.selected_contact_index as i32 + delta;
            if next < 0 {
                next = 0;
            }
            if next > max {
                next = max;
            }
            model.selected_contact_index = next as usize;
            for (idx, row) in model.contacts.iter_mut().enumerate() {
                row.selected = idx == model.selected_contact_index;
            }
        }
        UiScreen::Chat => {
            model.move_channel_selection(delta);
        }
        UiScreen::Notifications => {
            if model.notifications.is_empty() {
                model.selected_notification_index = 0;
                return;
            }
            let max = model.notifications.len() as i32 - 1;
            let mut next = model.selected_notification_index as i32 + delta;
            if next < 0 {
                next = 0;
            }
            if next > max {
                next = max;
            }
            model.selected_notification_index = next as usize;
        }
        UiScreen::Neighborhood => {}
    }
}

fn handle_horizontal(model: &mut UiModel, _delta: i32) {
    if model.screen == UiScreen::Contacts {
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
        AddDeviceWizardStep, CreateChannelWizardStep, ModalState, ThresholdWizardStep, UiModel,
        UiScreen,
    };

    #[test]
    fn contacts_invite_shortcut_opens_invite_modal() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Contacts);
        apply_text_keys(&mut model, "n", &clipboard);

        assert!(matches!(model.modal, Some(ModalState::CreateInvitation)));
    }

    #[test]
    fn create_invitation_modal_copy_shortcut_writes_clipboard() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.modal = Some(ModalState::CreateInvitation);
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

        model.modal = Some(ModalState::AcceptInvitation);
        model.modal_buffer = "1".to_string();
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.contacts.iter().any(|row| row.name == "Alice"));

        model.modal = Some(ModalState::AcceptInvitation);
        model.modal_buffer = "2".to_string();
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.contacts.iter().any(|row| row.name == "Carol"));
    }

    #[test]
    fn neighborhood_new_home_shortcut_opens_modal() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Neighborhood);
        apply_text_keys(&mut model, "n", &clipboard);

        assert!(matches!(model.modal, Some(ModalState::CreateHome)));
    }

    #[test]
    fn chat_shortcuts_open_expected_actions() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);

        apply_text_keys(&mut model, "n", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::CreateChannel)));

        // Close modal and ensure typing shortcut enters input mode.
        model.modal = None;
        apply_text_keys(&mut model, "i", &clipboard);
        assert!(model.input_mode);
    }

    #[test]
    fn create_channel_modal_uses_multistep_wizard_flow() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Chat);
        model.ensure_contact("Bob");
        model.ensure_contact("Carol");

        apply_text_keys(&mut model, "n", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::CreateChannel)));
        assert_eq!(model.modal_hint, "New Chat Group — Step 1 of 3");
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Details);

        apply_text_keys(&mut model, "team-room", &clipboard);
        apply_named_key(&mut model, "tab", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Details);

        apply_text_keys(&mut model, "bootstrap-topic", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Members);
        assert_eq!(model.modal_hint, "New Chat Group — Step 2 of 3");

        apply_named_key(&mut model, "down", 1, &clipboard);
        apply_text_keys(&mut model, " ", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(
            model.create_channel_step,
            CreateChannelWizardStep::Threshold
        );
        assert_eq!(model.modal_hint, "New Chat Group — Step 3 of 3");

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal.is_none());
        assert!(model.channels.iter().any(|row| row.name == "team-room"));
        assert_eq!(model.selected_channel_topic(), "bootstrap-topic");
    }

    #[test]
    fn create_channel_enter_from_name_advances_to_members() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Chat);

        apply_text_keys(&mut model, "n", &clipboard);
        apply_text_keys(&mut model, "demo-trio-room", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Members);
        assert_eq!(model.modal_hint, "New Chat Group — Step 2 of 3");
    }

    #[test]
    fn chat_enter_keeps_insert_mode_after_sending_message() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);
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

        model.set_screen(UiScreen::Chat);
        model.input_mode = true;
        model.input_buffer = "/nhlink home".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let toast = model.toast.expect("nhlink should emit a toast");
        assert!(toast.message.contains("status=denied"));
        assert!(toast.message.contains("reason=permission_denied"));
        assert!(toast.message.contains("consistency=accepted"));
    }

    #[test]
    fn chat_pin_command_reports_permission_denied() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);
        model.input_mode = true;
        model.input_buffer = "/pin msg-1".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let toast = model.toast.expect("pin should emit a toast");
        assert!(toast.message.contains("status=denied"));
        assert!(toast.message.contains("reason=permission_denied"));
    }

    #[test]
    fn chat_mode_minus_reports_enforced_success() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);
        model.input_mode = true;
        model.input_buffer = "/mode slash-lab -m".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let toast = model.toast.expect("mode -m should emit a toast");
        assert!(toast.message.contains("status=ok"));
        assert!(toast.message.contains("consistency=enforced"));
    }

    #[test]
    fn demo_trio_channel_synthesizes_alice_and_carol_replies() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);
        model.select_channel_by_name("demo-trio-room");
        model.input_mode = true;
        model.input_buffer = "demo-e2e-trio-token".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        assert!(model.messages.iter().any(|msg| msg.contains("Alice")));
        assert!(model.messages.iter().any(|msg| msg.contains("Carol")));
    }

    #[test]
    fn chat_unknown_command_reports_not_found_reason() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Chat);
        model.input_mode = true;
        model.input_buffer = "/unknowncmd".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let toast = model.toast.expect("unknown command should emit a toast");
        assert!(toast.message.contains("status=invalid"));
        assert!(toast.message.contains("reason=not_found"));
    }

    #[test]
    fn settings_shortcuts_open_or_toast_expected_actions() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Settings);

        model.settings_index = 0;
        apply_text_keys(&mut model, "e", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::EditNickname)));
        model.modal = None;

        model.settings_index = 1;
        apply_text_keys(&mut model, "t", &clipboard);
        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Add contacts first before setting up guardians")
        );

        model.settings_index = 2;
        apply_text_keys(&mut model, "s", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::RequestRecovery)));
        model.modal = None;

        model.settings_index = 3;
        apply_text_keys(&mut model, "a", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::AddDeviceStep1)));
        model.modal = None;
        apply_text_keys(&mut model, "i", &clipboard);
        assert!(matches!(
            model.modal,
            Some(ModalState::ImportDeviceEnrollmentCode)
        ));
        model.modal = None;
        apply_text_keys(&mut model, "r", &clipboard);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Cannot remove the current device")
        );

        model.settings_index = 4;
        apply_text_keys(&mut model, "s", &clipboard);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Only one authority available")
        );
        apply_text_keys(&mut model, "m", &clipboard);
        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("MFA requires at least 2 devices, but only 1 available")
        );
    }

    #[test]
    fn settings_remove_device_toast_repeats_with_new_event_key() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Settings);
        model.settings_index = 3;

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

        model.set_screen(UiScreen::Settings);
        model.settings_index = 3;
        model.has_secondary_device = true;
        model.set_secondary_device_name(Some("Laptop".to_string()));

        apply_text_keys(&mut model, "r", &clipboard);
        assert!(matches!(
            model.modal,
            Some(ModalState::SelectDeviceToRemove)
        ));
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(model.modal, Some(ModalState::ConfirmRemoveDevice)));
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
        model.set_screen(UiScreen::Settings);
        model.settings_index = 1;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");
        model.ensure_contact("Carol");

        apply_text_keys(&mut model, "t", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::GuardianSetup)));
        assert_eq!(model.guardian_wizard_step, ThresholdWizardStep::Selection);

        apply_named_key(&mut model, "down", 2, &clipboard);
        apply_text_keys(&mut model, " ", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.guardian_wizard_step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.guardian_wizard_step, ThresholdWizardStep::Ceremony);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Guardian ceremony started! Waiting for 2-of-3 guardians to respond")
        );
    }

    #[test]
    fn mfa_setup_wizard_advances_through_steps() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 4;
        model.has_secondary_device = true;

        apply_text_keys(&mut model, "m", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::MfaSetup)));
        assert_eq!(model.mfa_wizard_step, ThresholdWizardStep::Selection);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.mfa_wizard_step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.mfa_wizard_step, ThresholdWizardStep::Ceremony);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Multifactor ceremony started (2-of-2)")
        );
    }

    #[test]
    fn settings_add_device_wizard_requires_name_then_generates_code() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 3;

        apply_text_keys(&mut model, "a", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::AddDeviceStep1)));
        assert_eq!(model.modal_hint, "Add Device — Step 1 of 3");

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(model.modal, Some(ModalState::AddDeviceStep1)));
        assert_eq!(model.add_device_step, AddDeviceWizardStep::Name);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Device name is required")
        );

        apply_text_keys(&mut model, "Laptop", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(model.modal, Some(ModalState::AddDeviceStep1)));
        assert_eq!(model.add_device_step, AddDeviceWizardStep::ShareCode);
        assert_eq!(model.modal_hint, "Add Device — Step 2 of 3");
        assert!(!model.add_device_enrollment_code.is_empty());
    }

    #[test]
    fn settings_add_device_wizard_can_copy_generated_code() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 3;

        apply_text_keys(&mut model, "aPhone\n", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        apply_text_keys(&mut model, "c", &clipboard);

        assert_eq!(clipboard.read(), model.add_device_enrollment_code);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Copied to clipboard")
        );
    }

    #[test]
    fn request_recovery_requires_guardians_like_tui() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 2;

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(model.modal, Some(ModalState::RequestRecovery)));
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Set up guardians first before requesting recovery")
        );
    }

    #[test]
    fn request_recovery_starts_when_guardians_available() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 2;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");
        model.guardian_threshold_k = 2;

        apply_named_key(&mut model, "enter", 1, &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert!(model.modal.is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Recovery process started")
        );
    }

    #[test]
    fn create_channel_escape_steps_back_like_tui() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Chat);
        model.ensure_contact("Alice");

        apply_text_keys(&mut model, "nroom\n", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(
            model.create_channel_step,
            CreateChannelWizardStep::Threshold
        );

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Members);
        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Details);
    }

    #[test]
    fn guardian_setup_escape_from_threshold_returns_selection() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 1;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");

        apply_text_keys(&mut model, "t", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.guardian_wizard_step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(model.guardian_wizard_step, ThresholdWizardStep::Selection);
    }

    #[test]
    fn mfa_setup_escape_from_threshold_returns_selection() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(UiScreen::Settings);
        model.settings_index = 4;
        model.has_secondary_device = true;

        apply_text_keys(&mut model, "m", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.mfa_wizard_step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(model.mfa_wizard_step, ThresholdWizardStep::Selection);
    }
}

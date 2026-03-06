//! Keyboard input handling and navigation logic.
//!
//! Processes keyboard events to navigate screens, update selections, handle text
//! input, and dispatch commands across the UI model state machine.

use crate::clipboard::ClipboardPort;
use crate::model::{
    AccessDepth, ChannelRow, CreateChannelWizardStep, ModalState, ToastState, UiModel, UiScreen,
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
    for _ in 0..repeat {
        match key.trim().to_ascii_lowercase().as_str() {
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
        if modal_accepts_text(modal) {
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
        'c' => {
            if let Some(code) = model.last_invite_code.clone() {
                clipboard.write(&code);
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "Copied to clipboard".to_string(),
                });
                return;
            }
        }
        'y' => {
            if let Some(code) = model.last_invite_code.clone() {
                clipboard.write(&code);
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "Copied to clipboard".to_string(),
                });
                return;
            }
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
            model.modal = Some(ModalState::CreateChannel);
            model.reset_create_channel_wizard();
            model.modal_buffer.clear();
            model.modal_hint = "New Chat Group".to_string();
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
            model.modal_hint = "home invitation".to_string();
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
            model.modal = Some(ModalState::GuardianSetup);
            model.modal_hint = "Select guardians".to_string();
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
            model.modal_hint = "home invitation".to_string();
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
            model.modal_hint = "Access Override".to_string();
        }
        'p' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal = Some(ModalState::CapabilityConfig);
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
            model.modal = Some(ModalState::GuardianSetup);
            model.modal_hint = "Guardian Setup".to_string();
        }
        'a' if model.settings_index == 3 => {
            model.modal = Some(ModalState::AddDeviceStep1);
            model.modal_hint = "Add Device — Step 1 of 3".to_string();
        }
        'i' if model.settings_index == 3 => {
            model.modal = Some(ModalState::ImportDeviceEnrollmentCode);
            model.modal_hint = "Import Device Enrollment Code".to_string();
        }
        'r' if model.settings_index == 3 => {
            set_toast(model, '✗', "Cannot remove the current device");
        }
        's' if model.settings_index == 2 => {
            model.modal = Some(ModalState::RequestRecovery);
            model.modal_hint = "Request Recovery".to_string();
        }
        's' if model.settings_index == 4 => {
            set_toast(
                model,
                'ℹ',
                "Cannot switch authority: only one authority available",
            );
        }
        'm' if model.settings_index == 4 => {
            set_toast(
                model,
                '✗',
                "Cannot configure multifactor: requires at least 2 devices",
            );
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
                model.modal = Some(ModalState::GuardianSetup);
                model.modal_hint = "Guardian Setup".to_string();
            }
            2 => {
                model.modal = Some(ModalState::RequestRecovery);
                model.modal_hint = "Request Recovery".to_string();
            }
            3 => {
                model.modal = Some(ModalState::AddDeviceStep1);
                model.modal_hint = "Add Device — Step 1 of 3".to_string();
            }
            4 => {
                set_toast(
                    model,
                    '✗',
                    "Cannot configure multifactor: requires at least 2 devices",
                );
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
                model.ensure_contact("Alice");
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
        ModalState::CreateChannel => {
            match model.create_channel_step {
                CreateChannelWizardStep::Name => {
                    model.create_channel_name = model
                        .modal_buffer
                        .trim()
                        .trim_start_matches('#')
                        .to_string();
                    model.create_channel_step = CreateChannelWizardStep::Topic;
                    model.modal_buffer = model.create_channel_topic.clone();
                    model.modal_hint = "New Chat Group".to_string();
                }
                CreateChannelWizardStep::Topic => {
                    model.create_channel_topic = model.modal_buffer.trim().to_string();
                    model.create_channel_step = CreateChannelWizardStep::InviteContacts;
                    model.modal_buffer = model.create_channel_invitee.clone();
                    model.modal_hint = "Invite Contacts".to_string();
                }
                CreateChannelWizardStep::InviteContacts => {
                    model.create_channel_invitee = model.modal_buffer.trim().to_string();
                    model.create_channel_step = CreateChannelWizardStep::Threshold;
                    model.modal_buffer = model.create_channel_threshold.to_string();
                    model.modal_hint = "Threshold".to_string();
                }
                CreateChannelWizardStep::Threshold => {
                    if let Ok(value) = model.modal_buffer.trim().parse::<u8>() {
                        model.create_channel_threshold = value.max(1);
                    }
                    let channel = model.create_channel_name.trim().to_string();
                    if !channel.is_empty() {
                        model.select_channel_by_name(&channel);
                        if !model.create_channel_topic.trim().is_empty() {
                            model.set_selected_channel_topic(model.create_channel_topic.clone());
                        }
                        model.toast = Some(ToastState {
                            icon: '✓',
                            message: format!("Created '{channel}'."),
                        });
                    }
                    dismiss_modal(model);
                }
            }
        }
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
        ModalState::GuardianSetup => {
            set_toast(model, 'ℹ', "guardians for recovery");
            dismiss_modal(model);
        }
        ModalState::RequestRecovery => {
            set_toast(model, '✓', "Recovery request sent to guardians");
            dismiss_modal(model);
        }
        ModalState::AddDeviceStep1 => {
            set_toast(model, '✓', "invitation sent");
            dismiss_modal(model);
        }
        ModalState::ImportDeviceEnrollmentCode => {
            set_toast(model, '✓', "membership updated");
            dismiss_modal(model);
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
        ModalState::AccessOverride => {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "Access override preview".to_string(),
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
}

fn handle_escape(model: &mut UiModel) {
    if model.input_mode {
        model.input_mode = false;
        model.input_buffer.clear();
        return;
    }
    if model.modal.is_some() {
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
}

fn handle_modal_tab(model: &mut UiModel, reverse: bool) -> bool {
    if !matches!(model.modal, Some(ModalState::CreateChannel)) {
        return false;
    }

    if reverse {
        match model.create_channel_step {
            CreateChannelWizardStep::Name => {}
            CreateChannelWizardStep::Topic => {
                model.create_channel_topic = model.modal_buffer.clone();
                model.create_channel_step = CreateChannelWizardStep::Name;
                model.modal_buffer = model.create_channel_name.clone();
                model.modal_hint = "New Chat Group".to_string();
            }
            CreateChannelWizardStep::InviteContacts => {
                model.create_channel_invitee = model.modal_buffer.clone();
                model.create_channel_step = CreateChannelWizardStep::Topic;
                model.modal_buffer = model.create_channel_topic.clone();
                model.modal_hint = "New Chat Group".to_string();
            }
            CreateChannelWizardStep::Threshold => {
                if let Ok(value) = model.modal_buffer.trim().parse::<u8>() {
                    model.create_channel_threshold = value.max(1);
                }
                model.create_channel_step = CreateChannelWizardStep::InviteContacts;
                model.modal_buffer = model.create_channel_invitee.clone();
                model.modal_hint = "Invite Contacts".to_string();
            }
        }
        return true;
    }

    match model.create_channel_step {
        CreateChannelWizardStep::Name => {
            model.create_channel_name = model
                .modal_buffer
                .trim()
                .trim_start_matches('#')
                .to_string();
            model.create_channel_step = CreateChannelWizardStep::Topic;
            model.modal_buffer = model.create_channel_topic.clone();
            model.modal_hint = "New Chat Group".to_string();
        }
        CreateChannelWizardStep::Topic => {
            model.create_channel_topic = model.modal_buffer.trim().to_string();
            model.create_channel_step = CreateChannelWizardStep::InviteContacts;
            model.modal_buffer = model.create_channel_invitee.clone();
            model.modal_hint = "Invite Contacts".to_string();
        }
        CreateChannelWizardStep::InviteContacts => {
            model.create_channel_invitee = model.modal_buffer.trim().to_string();
            model.create_channel_step = CreateChannelWizardStep::Threshold;
            model.modal_buffer = model.create_channel_threshold.to_string();
            model.modal_hint = "Threshold".to_string();
        }
        CreateChannelWizardStep::Threshold => {}
    }
    true
}

fn modal_accepts_text(modal: ModalState) -> bool {
    matches!(
        modal,
        ModalState::CreateInvitation
            | ModalState::AcceptInvitation
            | ModalState::CreateHome
            | ModalState::CreateChannel
            | ModalState::SetChannelTopic
            | ModalState::EditNickname
            | ModalState::ImportDeviceEnrollmentCode
    )
}

fn backspace(model: &mut UiModel) {
    if model.input_mode {
        model.input_buffer.pop();
    } else if model.modal.is_some() {
        model.modal_buffer.pop();
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
    use crate::clipboard::MemoryClipboard;
    use crate::model::{CreateChannelWizardStep, ModalState, UiModel, UiScreen};

    #[test]
    fn contacts_invite_shortcut_opens_invite_modal() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(UiScreen::Contacts);
        apply_text_keys(&mut model, "n", &clipboard);

        assert!(matches!(model.modal, Some(ModalState::CreateInvitation)));
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

        apply_text_keys(&mut model, "n", &clipboard);
        assert!(matches!(model.modal, Some(ModalState::CreateChannel)));
        assert_eq!(model.modal_hint, "New Chat Group");
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Name);

        apply_text_keys(&mut model, "team-room", &clipboard);
        apply_named_key(&mut model, "tab", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Topic);

        apply_text_keys(&mut model, "bootstrap-topic", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::InviteContacts);
        assert_eq!(model.modal_hint, "Invite Contacts");

        apply_text_keys(&mut model, "bob", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(model.create_channel_step, CreateChannelWizardStep::Threshold);
        assert_eq!(model.modal_hint, "Threshold");

        apply_text_keys(&mut model, "2", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal.is_none());
        assert!(model.channels.iter().any(|row| row.name == "team-room"));
        assert_eq!(model.selected_channel_topic(), "bootstrap-topic");
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
        assert!(matches!(model.modal, Some(ModalState::GuardianSetup)));
        model.modal = None;

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
            Some("Cannot switch authority: only one authority available")
        );
        apply_text_keys(&mut model, "m", &clipboard);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Cannot configure multifactor: requires at least 2 devices")
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
}

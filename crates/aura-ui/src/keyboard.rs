use crate::clipboard::ClipboardPort;
use crate::model::{ChannelRow, ModalState, ToastState, UiModel, UiScreen};
use aura_app::ui::types::parse_chat_command;

pub fn apply_text_keys(model: &mut UiModel, keys: &str, clipboard: &dyn ClipboardPort) {
    for ch in keys.chars() {
        match ch {
            '\n' | '\r' => handle_enter(model, clipboard),
            '\u{08}' | '\u{7f}' => backspace(model),
            '\u{1b}' => handle_escape(model),
            _ => apply_char(model, ch),
        }
    }
}

pub fn apply_named_key(model: &mut UiModel, key: &str, clipboard: &dyn ClipboardPort) {
    match key.trim().to_ascii_lowercase().as_str() {
        "enter" => handle_enter(model, clipboard),
        "esc" => handle_escape(model),
        "tab" => cycle_screen(model),
        "up" => move_selection(model, -1),
        "down" => move_selection(model, 1),
        "backspace" => backspace(model),
        _ => {}
    }
}

fn apply_char(model: &mut UiModel, ch: char) {
    if ch.is_control() {
        return;
    }

    if model.input_mode {
        model.input_buffer.push(ch);
        return;
    }

    if model.modal.is_some() {
        model.modal_buffer.push(ch);
        return;
    }

    match ch {
        '1' => model.screen = UiScreen::Neighborhood,
        '2' => model.screen = UiScreen::Chat,
        '3' => model.screen = UiScreen::Contacts,
        '4' => model.screen = UiScreen::Notifications,
        '5' => model.screen = UiScreen::Settings,
        'i' if model.screen == UiScreen::Chat => {
            model.input_mode = true;
            model.input_buffer.clear();
        }
        'n' if model.screen == UiScreen::Contacts => {
            model.modal = Some(ModalState::CreateInvitation);
            model.modal_buffer.clear();
            model.toast = None;
        }
        'a' if model.screen == UiScreen::Contacts => {
            model.modal = Some(ModalState::AcceptInvitation);
            model.modal_buffer.clear();
            model.toast = None;
        }
        'e' if model.screen == UiScreen::Contacts => {
            model.modal = Some(ModalState::AcceptInvitation);
            model.modal_buffer.clear();
        }
        'n' if model.screen == UiScreen::Neighborhood => {
            model.modal = Some(ModalState::CreateHome);
            model.modal_buffer.clear();
        }
        'c' => {
            if let Some(code) = model.last_invite_code.clone() {
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Copied invitation code {code}"),
                });
            }
        }
        _ => {}
    }
}

fn handle_enter(model: &mut UiModel, clipboard: &dyn ClipboardPort) {
    if model.input_mode {
        let text = model.input_buffer.trim().to_string();
        model.input_mode = false;
        model.input_buffer.clear();
        if text.is_empty() {
            return;
        }
        submit_chat_input(model, &text);
        return;
    }

    if let Some(modal) = model.modal.clone() {
        match modal {
            ModalState::CreateInvitation => {
                model.invite_counter = model.invite_counter.saturating_add(1);
                let code = format!("INVITE-{}", model.invite_counter);
                model.last_invite_code = Some(code.clone());
                clipboard.write(&code);
                model.ensure_contact("Contact-1");
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Invitation Created {code}"),
                });
                model.logs.push("created invitation".to_string());
                model.modal = None;
                model.modal_buffer.clear();
            }
            ModalState::AcceptInvitation => {
                let value = model.modal_buffer.trim().to_string();
                if !value.is_empty() {
                    model.ensure_contact("Contact-1");
                    model.toast = Some(ToastState {
                        icon: '✓',
                        message: "Invitation accepted".to_string(),
                    });
                }
                model.modal = None;
                model.modal_buffer.clear();
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
                model.modal = None;
                model.modal_buffer.clear();
            }
        }
        return;
    }

    if model.screen == UiScreen::Neighborhood {
        if let Some(selected_home) = model.selected_home.clone() {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: format!("Selected home: {selected_home}"),
            });
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
                                model.channels.push(ChannelRow {
                                    name: "general".to_string(),
                                    selected: true,
                                });
                            } else if let Some(first) = model.channels.first_mut() {
                                first.selected = true;
                            }
                        }
                        model.toast = Some(command_toast('✓', "ok", "none", "replicated", "left channel"));
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
                    aura_app::ui::types::ChatCommand::Nick { .. } => {
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "accepted",
                            &format!("command {command_name} applied"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Msg { target: _, text }
                    | aura_app::ui::types::ChatCommand::Me { action: text } => {
                        model.messages.push(text.clone());
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "accepted",
                            &format!("command {command_name} applied"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Topic { text } => {
                        model.messages.push(text.clone());
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
                            &format!("command {command_name} applied"),
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
                model.toast = Some(command_toast(
                    '✗',
                    "invalid",
                    "invalid_argument",
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
        model.modal = None;
        model.modal_buffer.clear();
        return;
    }
    model.toast = None;
}

fn backspace(model: &mut UiModel) {
    if model.input_mode {
        model.input_buffer.pop();
    } else if model.modal.is_some() {
        model.modal_buffer.pop();
    }
}

fn cycle_screen(model: &mut UiModel) {
    model.screen = match model.screen {
        UiScreen::Neighborhood => UiScreen::Chat,
        UiScreen::Chat => UiScreen::Contacts,
        UiScreen::Contacts => UiScreen::Notifications,
        UiScreen::Notifications => UiScreen::Settings,
        UiScreen::Settings => UiScreen::Neighborhood,
    };
}

fn move_selection(model: &mut UiModel, delta: i32) {
    match model.screen {
        UiScreen::Settings => {
            let count = 5_i32;
            let current = model.settings_index as i32;
            let mut next = current + delta;
            if next < 0 {
                next = 0;
            }
            if next >= count {
                next = count - 1;
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
        _ => {}
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
        message: format!(
            "{detail} status={status} reason={reason} consistency={consistency}"
        ),
    }
}

use super::wizard::open_create_channel_wizard;
use crate::model::{ActiveModal, ChannelRow, EditChannelInfoModalState, ToastState, UiModel};
use aura_app::ui::types::parse_chat_command;
use aura_app::views::chat::{NOTE_TO_SELF_CHANNEL_NAME, NOTE_TO_SELF_CHANNEL_TOPIC};

pub(super) fn handle_chat_char(model: &mut UiModel, ch: char) {
    match ch {
        'i' => {
            model.input_mode = true;
            model.input_buffer.clear();
        }
        'n' => {
            open_create_channel_wizard(model);
        }
        'e' => {
            let channel_name = model
                .selected_channel_name()
                .unwrap_or(NOTE_TO_SELF_CHANNEL_NAME)
                .to_string();
            let topic = model.selected_channel_topic().to_string();
            model.modal_hint = "Edit Channel".to_string();
            model.active_modal = Some(ActiveModal::EditChannelInfo(EditChannelInfoModalState {
                name: channel_name,
                topic,
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

pub(super) fn submit_chat_input(model: &mut UiModel, text: &str) {
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
                                model.channels.push(ChannelRow {
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

pub(super) fn ensure_dm_channel(model: &mut UiModel) -> String {
    ensure_named_channel(model, "dm", String::new())
}

pub(crate) fn ensure_named_channel(
    model: &mut UiModel,
    channel_name: &str,
    topic: String,
) -> String {
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

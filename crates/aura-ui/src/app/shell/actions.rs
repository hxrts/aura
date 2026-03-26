use super::*;
pub(crate) fn selected_home_id_for_modal(
    runtime: &NeighborhoodRuntimeView,
    model: &UiModel,
) -> Option<String> {
    model
        .selected_home_id()
        .map(ToString::to_string)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            runtime
                .homes
                .iter()
                .find(|home| Some(home.name.as_str()) == model.selected_home_name())
                .map(|home| home.id.clone())
                .filter(|id| !id.is_empty())
        })
        .or_else(|| (!runtime.active_home_id.is_empty()).then(|| runtime.active_home_id.clone()))
}

pub(crate) fn submit_runtime_chat_input(
    controller: Arc<UiController>,
    channel_name: String,
    input_text: String,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let trimmed = input_text.trim().to_string();
    if trimmed.is_empty() {
        return false;
    }

    let app_core = controller.app_core().clone();
    let controller_for_task = controller.clone();
    spawn_ui(async move {
        let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
            Ok(value) => value,
            Err(error) => {
                controller_for_task.runtime_error_toast(error.to_string());
                rerender();
                return;
            }
        };

        let result: Result<Option<String>, aura_core::AuraError> = if let Some(command_input) =
            trimmed.strip_prefix('/')
        {
            let raw = format!("/{command_input}");
            match parse_chat_command(&raw) {
                Ok(ChatCommand::Join { channel }) => {
                    controller_for_task.push_log(&format!("chat_join: start channel={channel}"));
                    messaging_workflows::join_channel_by_name(&app_core, &channel)
                        .await
                        .map(|channel_id| {
                            controller_for_task.select_channel_by_id(&channel_id);
                            controller_for_task.push_runtime_fact(RuntimeFact::ChannelJoined {
                                channel: Some(ChannelFactKey::identified(channel_id.clone())),
                                source: Some("join_command".to_string()),
                            });
                            controller_for_task.push_log(&format!(
                                "chat_join: success channel={channel} selected_id={channel_id}"
                            ));
                            Some(format!("joined #{}", channel.trim_start_matches('#')))
                        })
                }
                Ok(ChatCommand::Leave) => {
                    messaging_workflows::leave_channel_by_name(&app_core, &channel_name)
                        .await
                        .map(|_| Some("left channel".to_string()))
                }
                Ok(ChatCommand::Topic { text }) => messaging_workflows::set_topic_by_name(
                    &app_core,
                    &channel_name,
                    &text,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("topic updated".to_string())),
                Ok(ChatCommand::Me { action }) => messaging_workflows::send_action_by_name(
                    &app_core,
                    &channel_name,
                    &action,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("action sent".to_string())),
                Ok(ChatCommand::Msg { target, text }) => messaging_workflows::send_direct_message(
                    &app_core,
                    &target,
                    &text,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("direct message sent".to_string())),
                Ok(ChatCommand::Nick { name }) => {
                    settings_workflows::update_nickname(&app_core, name)
                        .await
                        .map(|_| Some("nickname updated".to_string()))
                }
                Ok(ChatCommand::Invite { target }) => messaging_workflows::invite_user_to_channel(
                    &app_core,
                    &target,
                    &channel_name,
                    None,
                    None,
                )
                .await
                .map(|_| Some("invitation sent".to_string())),
                Ok(ChatCommand::Who) => {
                    query_workflows::list_participants(&app_core, &channel_name)
                        .await
                        .map(|participants| {
                            Some(if participants.is_empty() {
                                "No participants".to_string()
                            } else {
                                participants.join(", ")
                            })
                        })
                }
                Ok(ChatCommand::Whois { target }) => {
                    query_workflows::get_user_info(&app_core, &target)
                        .await
                        .map(|contact| {
                            let id = contact.id.to_string();
                            let name = if !contact.nickname.is_empty() {
                                contact.nickname
                            } else if let Some(value) = contact.nickname_suggestion {
                                value
                            } else {
                                id.chars().take(8).collect::<String>() + "..."
                            };
                            Some(format!("User: {name} ({id})"))
                        })
                }
                Ok(ChatCommand::Help { command }) => Ok(Some(match command {
                    Some(command_name) => {
                        if let Some(help) = command_help(&command_name) {
                            format!(
                                "/{name} {syntax} — {description}",
                                name = help.name,
                                syntax = help.syntax,
                                description = help.description
                            )
                        } else {
                            format!("Unknown command: {command_name}")
                        }
                    }
                    None => {
                        let commands = all_command_help()
                            .into_iter()
                            .take(8)
                            .map(|help| format!("/{}", help.name))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("Common commands: {commands}. Use /help <command> for details.")
                    }
                })),
                Ok(ChatCommand::Neighborhood { name }) => {
                    context_workflows::create_neighborhood(&app_core, name)
                        .await
                        .map(|_| Some("neighborhood updated".to_string()))
                }
                Ok(ChatCommand::NhAdd { home_id }) => {
                    context_workflows::add_home_to_neighborhood(&app_core, &home_id)
                        .await
                        .map(|_| Some("home added to neighborhood".to_string()))
                }
                Ok(ChatCommand::NhLink { home_id }) => {
                    context_workflows::link_home_one_hop_link(&app_core, &home_id)
                        .await
                        .map(|_| Some("home one-hop link linked".to_string()))
                }
                Ok(ChatCommand::HomeInvite { target }) => {
                    let home_id = match context_workflows::current_home_id(&app_core).await {
                        Ok(home_id) => home_id.to_string(),
                        Err(error) => {
                            controller_for_task.runtime_error_toast(error.to_string());
                            rerender();
                            return;
                        }
                    };
                    let target_authority =
                        match query_workflows::resolve_contact(&app_core, &target).await {
                            Ok(contact) => contact.id,
                            Err(_) => match target.parse::<AuthorityId>() {
                                Ok(authority_id) => authority_id,
                                Err(error) => {
                                    controller_for_task.runtime_error_toast(format!(
                                        "Invalid authority id: {error}"
                                    ));
                                    rerender();
                                    return;
                                }
                            },
                        };
                    invitation_workflows::create_channel_invitation(
                        &app_core,
                        target_authority,
                        home_id,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                    .map(|_| Some("home invitation sent".to_string()))
                }
                Ok(ChatCommand::HomeAccept) => {
                    invitation_workflows::accept_pending_home_invitation(&app_core)
                        .await
                        .map(|_| Some("home invitation accepted".to_string()))
                }
                Ok(ChatCommand::Kick { target, reason }) => moderation_workflows::kick_user(
                    &app_core,
                    &channel_name,
                    &target,
                    reason.as_deref(),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("kick applied".to_string())),
                Ok(ChatCommand::Ban { target, reason }) => moderation_workflows::ban_user(
                    &app_core,
                    Some(&channel_name),
                    &target,
                    reason.as_deref(),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("ban applied".to_string())),
                Ok(ChatCommand::Unban { target }) => {
                    moderation_workflows::unban_user(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("unban applied".to_string()))
                }
                Ok(ChatCommand::Mute { target, duration }) => moderation_workflows::mute_user(
                    &app_core,
                    Some(&channel_name),
                    &target,
                    duration.map(|value| value.as_secs()),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("mute applied".to_string())),
                Ok(ChatCommand::Unmute { target }) => {
                    moderation_workflows::unmute_user(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("unmute applied".to_string()))
                }
                Ok(ChatCommand::Pin { message_id }) => {
                    moderation_workflows::pin_message(&app_core, &message_id)
                        .await
                        .map(|_| Some("message pinned".to_string()))
                }
                Ok(ChatCommand::Unpin { message_id }) => {
                    moderation_workflows::unpin_message(&app_core, &message_id)
                        .await
                        .map(|_| Some("message unpinned".to_string()))
                }
                Ok(ChatCommand::Op { target }) => {
                    moderator_workflows::grant_moderator(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("moderator granted".to_string()))
                }
                Ok(ChatCommand::Deop { target }) => {
                    moderator_workflows::revoke_moderator(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("moderator revoked".to_string()))
                }
                Ok(ChatCommand::Mode { channel, flags }) => {
                    settings_workflows::set_channel_mode(&app_core, channel, flags)
                        .await
                        .map(|_| Some("channel mode updated".to_string()))
                }
                Err(error) => Err(aura_core::AuraError::invalid(error.to_string())),
            }
        } else {
            messaging_workflows::send_message_by_name(
                &app_core,
                &channel_name,
                &trimmed,
                timestamp_ms,
            )
            .await
            .map(|_| {
                controller_for_task.push_runtime_fact(RuntimeFact::MessageCommitted {
                    channel: ChannelFactKey::named(channel_name.clone()),
                    content: trimmed.clone(),
                });
                None
            })
        };

        match result {
            Ok(message) => {
                controller_for_task.clear_input_buffer();
                if let Some(message) = message {
                    controller_for_task.push_log(&format!("chat_command: ok message={message}"));
                    controller_for_task.info_toast(message);
                }
            }
            Err(error) => {
                controller_for_task.push_log(&format!("chat_command: error {error}"));
                controller_for_task.runtime_error_toast(error.to_string());
            }
        }
        rerender();
    });

    controller.clear_input_buffer();
    true
}

pub(crate) fn handle_runtime_character_shortcut(
    controller: Arc<UiController>,
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    key: &str,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    if model.input_mode || model.modal_state().is_some() {
        return false;
    }

    match (model.screen, key) {
        (ScreenId::Neighborhood, "m") => {
            let app_core = controller.app_core().clone();
            spawn_ui(async move {
                match context_workflows::create_neighborhood(&app_core, "Neighborhood".to_string())
                    .await
                {
                    Ok(_) => controller.info_toast("Neighborhood ready"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "v") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            spawn_ui(async move {
                match context_workflows::add_home_to_neighborhood(&app_core, &home_id).await {
                    Ok(_) => controller.info_toast("Home added to neighborhood"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "L") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            spawn_ui(async move {
                match context_workflows::link_home_one_hop_link(&app_core, &home_id).await {
                    Ok(_) => controller.info_toast("Direct one-hop link created"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        _ => false,
    }
}

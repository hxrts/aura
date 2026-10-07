//! Chat command handlers using the aura-chat service.
//!
//! Returns structured `CliOutput` for text and `--json` rendering.

use crate::cli::chat::ChatCommands;
use crate::command::confirm;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_agent::handlers::{ChatGroupId, ChatMessageId};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn confirmed(prompt: &str, assume_yes: bool) -> TerminalResult<()> {
    confirm(prompt, assume_yes).map_err(|e| TerminalError::Input(e.message))
}

/// Execute chat management commands through the ChatServiceApi
pub async fn handle_chat(
    ctx: &HandlerContext<'_>,
    command: &ChatCommands,
    assume_yes: bool,
) -> TerminalResult<CliOutput> {
    let agent = ctx.agent().ok_or_else(|| {
        TerminalError::Operation("Agent not available - please initialize an account first".into())
    })?;

    let chat = agent.chat()?;
    let authority_id = ctx.effect_context().authority_id();
    let mut output = CliOutput::new();

    match command {
        ChatCommands::Create { name, members } => {
            let group = chat
                .create_group(name, authority_id, members.clone())
                .await?;
            output.kv("Created chat group", &group.name);
            output.kv("ID", group.id.to_string());
        }

        ChatCommands::Send { group_id, message } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let msg = chat
                .send_message(&group_id, authority_id, message.clone())
                .await?;
            output.kv("Message sent", msg.id.to_string());
        }

        ChatCommands::History {
            group_id,
            limit,
            sender,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let history = chat.get_history(&group_id, Some(*limit), None).await?;
            let rows: Vec<Vec<String>> = history
                .iter()
                .filter(|msg| sender.map_or(true, |s| msg.sender_id == s))
                .map(|msg| vec![msg.sender_id.to_string(), msg.content.clone()])
                .collect();
            output.section(format!("Message History ({} messages)", rows.len()));
            output.table(&["Sender", "Message"], &rows);
        }

        ChatCommands::List => {
            let groups = chat.list_user_groups(&authority_id).await?;
            let rows: Vec<Vec<String>> = groups
                .iter()
                .map(|group| {
                    vec![
                        group.id.to_string(),
                        group.name.clone(),
                        group.members.len().to_string(),
                    ]
                })
                .collect();
            output.section(format!("Your Chat Groups ({})", rows.len()));
            output.table(&["ID", "Name", "Members"], &rows);
        }

        ChatCommands::Show {
            group_id,
            show_members,
            show_metadata,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let group = chat
                .get_group(&group_id)
                .await?
                .ok_or_else(|| TerminalError::NotFound(format!("Group not found: {group_id}")))?;

            output.section(&group.name);
            output.kv("ID", group.id.to_string());
            if let Some((context_id, channel_id)) = chat.group_transport_ids(&group_id).await? {
                output.kv("Context", context_id.to_string());
                output.kv("Channel", channel_id.to_string());
            }
            output.kv("Description", &group.description);
            output.kv("Created by", group.created_by.to_string());

            if *show_members {
                output.section("Members");
                let rows: Vec<Vec<String>> = group
                    .members
                    .iter()
                    .map(|m| vec![m.nickname_suggestion.clone(), format!("{:?}", m.role)])
                    .collect();
                output.table(&["Name", "Role"], &rows);
            }

            if *show_metadata && !group.metadata.is_empty() {
                output.section("Metadata");
                for (k, v) in &group.metadata {
                    output.kv(k, v);
                }
            }
        }

        ChatCommands::Invite {
            group_id,
            authority_id: member_to_add,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            chat.add_member(&group_id, authority_id, *member_to_add)
                .await?;
            output.println(format!("Added {member_to_add} to group {group_id}"));
        }

        ChatCommands::Leave { group_id } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            confirmed(&format!("Leave group {group_id}?"), assume_yes)?;
            chat.remove_member(&group_id, authority_id, authority_id)
                .await?;
            output.println(format!("Left group {group_id}"));
        }

        ChatCommands::Remove {
            group_id,
            member_id,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            confirmed(
                &format!("Remove {member_id} from group {group_id}?"),
                assume_yes,
            )?;
            chat.remove_member(&group_id, authority_id, *member_id)
                .await?;
            output.println(format!("Removed {member_id} from group {group_id}"));
        }

        ChatCommands::Update {
            group_id,
            name,
            description,
            metadata,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let meta_map: Option<HashMap<String, String>> = if metadata.is_empty() {
                None
            } else {
                let mut map = HashMap::new();
                for pair in metadata {
                    let (k, v) = pair.split_once('=').ok_or_else(|| {
                        TerminalError::Input(format!("metadata must be key=value, got {pair}"))
                    })?;
                    map.insert(k.to_string(), v.to_string());
                }
                Some(map)
            };
            let group = chat
                .update_group_details(
                    &group_id,
                    authority_id,
                    name.clone(),
                    description.clone(),
                    meta_map,
                )
                .await?;
            output.kv("Updated group", &group.name);
        }

        ChatCommands::Search {
            query,
            group_id,
            limit,
            sender,
        } => {
            let group_id = group_id.ok_or_else(|| {
                TerminalError::Input("specify a group with --group-id to search".into())
            })?;
            let group_id = ChatGroupId::from_uuid(group_id);
            let results = chat
                .search_messages(&group_id, query, *limit, sender.as_ref())
                .await?;
            let rows: Vec<Vec<String>> = results
                .iter()
                .map(|msg| vec![msg.id.to_string(), msg.content.clone()])
                .collect();
            output.section(format!("Search Results ({})", rows.len()));
            output.table(&["ID", "Message"], &rows);
        }

        ChatCommands::Edit {
            group_id,
            message_id,
            content,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let message_id = ChatMessageId::from_uuid(*message_id);
            let msg = chat
                .edit_message(&group_id, authority_id, &message_id, content)
                .await?;
            output.kv("Message updated", msg.id.to_string());
        }

        ChatCommands::Delete {
            group_id,
            message_id,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let message_id = ChatMessageId::from_uuid(*message_id);
            confirmed(&format!("Delete message {message_id}?"), assume_yes)?;
            chat.delete_message(&group_id, authority_id, &message_id)
                .await?;
            output.println("Message deleted");
        }

        ChatCommands::Export {
            group_id,
            output: path,
            format,
            include_system,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let history = chat.get_history(&group_id, None, None).await?;
            let filtered_history = history
                .into_iter()
                .filter(|message| *include_system || !message.is_system())
                .collect::<Vec<_>>();

            let body = match format.to_lowercase().as_str() {
                "json" => serde_json::to_string_pretty(&filtered_history).map_err(|error| {
                    TerminalError::Operation(format!("Failed to serialize chat export: {error}"))
                })?,
                "csv" => {
                    let mut rows = vec![
                        "message_id,group_id,sender_id,message_type,timestamp,reply_to,content"
                            .to_string(),
                    ];
                    for message in &filtered_history {
                        let content = message.content.replace('"', "\"\"");
                        let reply_to = message
                            .reply_to
                            .clone()
                            .map(|reply| reply.to_string())
                            .unwrap_or_default();
                        rows.push(format!(
                            "\"{}\",\"{}\",\"{}\",\"{:?}\",\"{:?}\",\"{}\",\"{}\"",
                            message.id,
                            message.group_id,
                            message.sender_id,
                            message.message_type,
                            message.timestamp,
                            reply_to,
                            content
                        ));
                    }
                    rows.join("\n")
                }
                "text" => filtered_history
                    .iter()
                    .map(|message| {
                        format!(
                            "[{:?}] {} {:?}: {}",
                            message.timestamp,
                            message.sender_id,
                            message.message_type,
                            message.content
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => {
                    return Err(TerminalError::Input(format!(
                        "Unsupported chat export format: {other}"
                    )));
                }
            };

            let output_path = Path::new(path);
            if let Some(parent) = output_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).map_err(|error| {
                    TerminalError::Operation(format!(
                        "Failed to create export directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
            fs::write(output_path, body).map_err(|error| {
                TerminalError::Operation(format!(
                    "Failed to write chat export {}: {error}",
                    output_path.display()
                ))
            })?;
            output.kv("Exported messages", filtered_history.len().to_string());
            output.kv("Group", group_id.to_string());
            output.kv("File", output_path.display().to_string());
        }
    }

    Ok(output)
}

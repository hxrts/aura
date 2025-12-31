//! Chat command handlers using the aura-chat service and effect system
//!
//! This module implements CLI handlers for chat functionality, integrating
//! with the agent-layer ChatService for group management and messaging.

use crate::cli::chat::ChatCommands;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::HandlerContext;
use aura_agent::handlers::{ChatGroupId, ChatMessageId};
use aura_agent::AuraEffectSystem;
use aura_core::effects::ConsoleEffects;
use std::collections::HashMap;

/// Execute chat management commands through the ChatService
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_chat(
    ctx: &HandlerContext<'_>,
    _effects: &AuraEffectSystem,
    command: &ChatCommands,
) -> TerminalResult<()> {
    let agent = ctx.agent().ok_or_else(|| {
        TerminalError::Operation("Agent not available - please initialize an account first".into())
    })?;

    let chat = agent.chat();
    let authority_id = ctx.effect_context().authority_id();

    match command {
        ChatCommands::Create {
            name,
            description,
            members,
        } => {
            let group = chat
                .create_group(name, authority_id, members.clone())
                .await?;

            ConsoleEffects::log_info(
                ctx.effects(),
                &format!("Created chat group: {} (ID: {})", group.name, group.id),
            )
            .await?;

            if let Some(desc) = description {
                ConsoleEffects::log_warn(
                    ctx.effects(),
                    &format!(
                        "Group descriptions are not yet fact-backed; ignoring provided description: {desc}"
                    ),
                )
                .await?;
            }
        }

        ChatCommands::Send {
            group_id,
            message,
            reply_to: _,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let msg = chat
                .send_message(&group_id, authority_id, message.clone())
                .await?;

            ConsoleEffects::log_info(ctx.effects(), &format!("Message sent (ID: {})", msg.id))
                .await?;
        }

        ChatCommands::History {
            group_id,
            limit,
            before: _,
            message_type: _,
            sender: _,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let history = chat.get_history(&group_id, Some(*limit), None).await?;

            if history.is_empty() {
                ConsoleEffects::log_info(ctx.effects(), "No messages in this group").await?;
            } else {
                ConsoleEffects::log_info(
                    ctx.effects(),
                    &format!("=== Message History ({} messages) ===", history.len()),
                )
                .await?;

                for msg in history {
                    let sender_short = msg.sender_id.to_string();
                    let sender_display = &sender_short[..12.min(sender_short.len())];
                    ConsoleEffects::log_info(
                        ctx.effects(),
                        &format!("[{}...] {}", sender_display, msg.content),
                    )
                    .await?;
                }
            }
        }

        ChatCommands::List => {
            let groups = chat.list_user_groups(&authority_id).await?;

            if groups.is_empty() {
                ConsoleEffects::log_info(ctx.effects(), "No chat groups found").await?;
            } else {
                ConsoleEffects::log_info(
                    ctx.effects(),
                    &format!("=== Your Chat Groups ({}) ===", groups.len()),
                )
                .await?;

                for group in groups {
                    ConsoleEffects::log_info(
                        ctx.effects(),
                        &format!(
                            "  {} - {} ({} members)",
                            group.id,
                            group.name,
                            group.members.len()
                        ),
                    )
                    .await?;
                }
            }
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

            ConsoleEffects::log_info(ctx.effects(), &format!("=== {} ===", group.name)).await?;
            ConsoleEffects::log_info(ctx.effects(), &format!("ID: {}", group.id)).await?;
            ConsoleEffects::log_info(
                ctx.effects(),
                &format!("Description: {}", group.description),
            )
            .await?;
            ConsoleEffects::log_info(ctx.effects(), &format!("Created by: {}", group.created_by))
                .await?;

            if *show_members {
                ConsoleEffects::log_info(ctx.effects(), "\nMembers:").await?;
                for member in &group.members {
                    ConsoleEffects::log_info(
                        ctx.effects(),
                        &format!("  - {} ({:?})", member.display_name, member.role),
                    )
                    .await?;
                }
            }

            if *show_metadata && !group.metadata.is_empty() {
                ConsoleEffects::log_info(ctx.effects(), "\nMetadata:").await?;
                for (k, v) in &group.metadata {
                    ConsoleEffects::log_info(ctx.effects(), &format!("  {k}: {v}")).await?;
                }
            }
        }

        ChatCommands::Invite {
            group_id,
            authority_id: member_to_add,
            role: _,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            chat.add_member(&group_id, authority_id, *member_to_add)
                .await?;

            ConsoleEffects::log_info(
                ctx.effects(),
                &format!("Added {member_to_add} to group {group_id}"),
            )
            .await?;
        }

        ChatCommands::Leave { group_id, force: _ } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            chat.remove_member(&group_id, authority_id, authority_id)
                .await?;

            ConsoleEffects::log_info(ctx.effects(), &format!("Left group {group_id}")).await?;
        }

        ChatCommands::Remove {
            group_id,
            member_id,
            force: _,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            chat.remove_member(&group_id, authority_id, *member_id)
                .await?;

            ConsoleEffects::log_info(
                ctx.effects(),
                &format!("Removed {member_id} from group {group_id}"),
            )
            .await?;
        }

        ChatCommands::Update {
            group_id,
            name,
            description,
            metadata,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);

            // Parse metadata key=value pairs
            let meta_map: Option<HashMap<String, String>> = if metadata.is_empty() {
                None
            } else {
                Some(
                    metadata
                        .iter()
                        .filter_map(|s| {
                            let parts: Vec<&str> = s.splitn(2, '=').collect();
                            if parts.len() == 2 {
                                Some((parts[0].to_string(), parts[1].to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
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

            ConsoleEffects::log_info(ctx.effects(), &format!("Updated group: {}", group.name))
                .await?;
        }

        ChatCommands::Search {
            query,
            group_id,
            limit,
            sender,
        } => {
            if let Some(gid) = group_id {
                let group_id = ChatGroupId::from_uuid(*gid);
                let results = chat
                    .search_messages(&group_id, query, *limit, sender.as_ref())
                    .await?;

                if results.is_empty() {
                    ConsoleEffects::log_info(ctx.effects(), "No messages found").await?;
                } else {
                    ConsoleEffects::log_info(
                        ctx.effects(),
                        &format!("=== Search Results ({}) ===", results.len()),
                    )
                    .await?;

                    for msg in results {
                        ConsoleEffects::log_info(
                            ctx.effects(),
                            &format!("[{}] {}", msg.id, msg.content),
                        )
                        .await?;
                    }
                }
            } else {
                ConsoleEffects::log_info(
                    ctx.effects(),
                    "Please specify a group ID with --group-id to search",
                )
                .await?;
            }
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

            ConsoleEffects::log_info(ctx.effects(), &format!("Message {} updated", msg.id)).await?;
        }

        ChatCommands::Delete {
            group_id,
            message_id,
            force: _,
        } => {
            let group_id = ChatGroupId::from_uuid(*group_id);
            let message_id = ChatMessageId::from_uuid(*message_id);
            chat.delete_message(&group_id, authority_id, &message_id)
                .await?;

            ConsoleEffects::log_info(ctx.effects(), "Message deleted").await?;
        }

        ChatCommands::Export {
            group_id,
            output,
            format: _,
            include_system: _,
        } => {
            // Export functionality requires additional infrastructure
            ConsoleEffects::log_info(
                ctx.effects(),
                &format!(
                    "Export functionality for group {group_id} to {output} not yet implemented"
                ),
            )
            .await?;
        }
    }

    Ok(())
}

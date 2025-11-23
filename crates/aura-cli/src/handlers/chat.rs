//! Chat command handlers using the aura-chat service and effect system
//!
//! This module implements CLI handlers for chat functionality, integrating
//! with the aura-chat service for group management and messaging.

use crate::commands::chat::ChatCommands;
use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_chat::{ChatGroupId, ChatMessageId, ChatService};
use aura_core::{effects::ConsoleEffects, identifiers::AuthorityId, AuraError};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use uuid::Uuid;

/// Execute chat management commands through the effect system
pub async fn handle_chat(
    ctx: &EffectContext,
    effect_system: &Arc<AuraEffectSystem>,
    command: &ChatCommands,
) -> Result<()> {
    // Create chat service with effect system
    let chat_service = ChatService::new(Arc::clone(effect_system));

    match command {
        ChatCommands::Create {
            name,
            description,
            members,
        } => {
            handle_create_group(
                ctx,
                &chat_service,
                effect_system,
                name,
                description.as_deref(),
                members,
            )
            .await
        }

        ChatCommands::Send {
            group_id,
            message,
            reply_to: _,
        } => handle_send_message(ctx, &chat_service, effect_system, group_id, message).await,

        ChatCommands::History {
            group_id,
            limit,
            before,
            message_type,
            sender,
        } => {
            handle_get_history(
                ctx,
                &chat_service,
                effect_system,
                group_id,
                *limit,
                before.as_deref(),
                message_type.as_deref(),
                sender.as_ref(),
            )
            .await
        }

        ChatCommands::List => handle_list_groups(ctx, &chat_service, effect_system).await,

        ChatCommands::Show {
            group_id,
            show_members,
            show_metadata,
        } => {
            handle_show_group(
                ctx,
                &chat_service,
                effect_system,
                group_id,
                *show_members,
                *show_metadata,
            )
            .await
        }

        ChatCommands::Invite {
            group_id,
            authority_id,
            role: _,
        } => handle_invite_member(ctx, &chat_service, effect_system, group_id, authority_id).await,

        ChatCommands::Leave { group_id, force: _ } => {
            handle_leave_group(ctx, &chat_service, effect_system, group_id).await
        }

        ChatCommands::Remove {
            group_id,
            member_id,
            force: _,
        } => handle_remove_member(ctx, &chat_service, effect_system, group_id, member_id).await,

        ChatCommands::Update {
            group_id,
            name,
            description,
            metadata,
        } => {
            handle_update_group(
                ctx,
                &chat_service,
                effect_system,
                group_id,
                name,
                description,
                metadata,
            )
            .await
        }

        ChatCommands::Search {
            query,
            group_id,
            limit,
            sender,
        } => {
            handle_search_messages(
                ctx,
                &chat_service,
                effect_system,
                query,
                group_id.as_ref(),
                *limit,
                sender.as_ref(),
            )
            .await
        }

        ChatCommands::Edit {
            group_id,
            message_id,
            content,
        } => {
            handle_edit_message(
                ctx,
                &chat_service,
                effect_system,
                group_id,
                message_id,
                content,
            )
            .await
        }

        ChatCommands::Delete {
            group_id,
            message_id,
            force: _,
        } => handle_delete_message(ctx, &chat_service, effect_system, group_id, message_id).await,

        ChatCommands::Export {
            group_id,
            output,
            format,
            include_system,
        } => {
            handle_export_history(
                ctx,
                &chat_service,
                effect_system,
                group_id,
                output,
                format,
                *include_system,
            )
            .await
        }
    }
}

/// Handle group creation command
async fn handle_create_group(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    name: &str,
    description: Option<&str>,
    members: &[AuthorityId],
) -> Result<()> {
    let creator_id = ctx.authority_id().clone();

    match chat_service
        .create_group(name, creator_id, members.to_vec())
        .await
    {
        Ok(group) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "✓ Created chat group '{}' with ID: {}",
                    group.name, group.id
                ),
            )
            .await?;

            if let Some(desc) = description {
                ConsoleEffects::log_info(effect_system, &format!("  Description: {}", desc))
                    .await?;
            }

            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "  Members: {} (including you as admin)",
                    group.member_count()
                ),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to create group: {}", e))
                .await?;
            anyhow::bail!("Group creation failed: {}", e);
        }
    }

    Ok(())
}

/// Handle message sending command
async fn handle_send_message(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    message_content: &str,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let sender_id = ctx.authority_id().clone();

    match chat_service
        .send_message(&group_id, sender_id, message_content.to_string())
        .await
    {
        Ok(message) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "✓ Message sent to group {} at {}",
                    group_id,
                    message.timestamp.format("%H:%M:%S")
                ),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to send message: {}", e))
                .await?;
            anyhow::bail!("Message sending failed: {}", e);
        }
    }

    Ok(())
}

/// Handle message history retrieval
async fn handle_get_history(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    limit: usize,
    before: Option<&str>,
    message_type: Option<&str>,
    sender: Option<&AuthorityId>,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let before_ts = if let Some(raw) = before {
        match DateTime::parse_from_rfc3339(raw) {
            Ok(ts) => Some(ts.with_timezone(&Utc)),
            Err(e) => {
                ConsoleEffects::log_error(
                    effect_system,
                    &format!("Invalid timestamp '{}': {}", raw, e),
                )
                .await?;
                None
            }
        }
    } else {
        None
    };

    let history = chat_service
        .get_history(&group_id, Some(limit), before_ts)
        .await?;

    let mut filtered = Vec::new();
    for msg in history {
        if let Some(filter_sender) = sender {
            if msg.sender_id != *filter_sender {
                continue;
            }
        }

        if let Some(filter_type) = message_type {
            let matches = match filter_type.to_lowercase().as_str() {
                "text" => matches!(msg.message_type, aura_chat::types::MessageType::Text),
                "system" => matches!(msg.message_type, aura_chat::types::MessageType::System),
                "edit" => matches!(msg.message_type, aura_chat::types::MessageType::Edit),
                "delete" => matches!(msg.message_type, aura_chat::types::MessageType::Delete),
                _ => true,
            };
            if !matches {
                continue;
            }
        }

        filtered.push(msg);
    }

    if filtered.is_empty() {
        ConsoleEffects::log_info(
            effect_system,
            &format!("📜 No messages found for group {}", group_id),
        )
        .await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        effect_system,
        &format!(
            "📜 Message history for group {} (showing {} entries)",
            group_id,
            filtered.len()
        ),
    )
    .await?;

    for message in filtered {
        let ts = message.timestamp.format("%Y-%m-%d %H:%M:%S");
        let sender_display = message.sender_id.to_string();
        let kind = format!("{:?}", message.message_type);
        let content = if message.message_type == aura_chat::types::MessageType::Delete {
            "<deleted>".to_string()
        } else {
            message.content.clone()
        };

        ConsoleEffects::log_info(
            effect_system,
            &format!("[{}][{}][{}] {}", ts, kind, sender_display, content),
        )
        .await?;
    }

    Ok(())
}

/// Handle group listing command
async fn handle_list_groups(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
) -> Result<()> {
    let authority_id = &ctx.authority_id();

    match chat_service.list_user_groups(authority_id).await {
        Ok(groups) => {
            if groups.is_empty() {
                ConsoleEffects::log_info(
                    effect_system,
                    "📭 No chat groups found. Use 'aura chat create' to create one.",
                )
                .await?;
            } else {
                ConsoleEffects::log_info(
                    effect_system,
                    &format!("📋 Your chat groups ({}):", groups.len()),
                )
                .await?;

                for group in groups {
                    ConsoleEffects::log_info(
                        effect_system,
                        &format!(
                            "  • {} (ID: {}, {} members)",
                            group.name,
                            group.id,
                            group.member_count()
                        ),
                    )
                    .await?;
                }
            }
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to list groups: {}", e))
                .await?;
            anyhow::bail!("Group listing failed: {}", e);
        }
    }

    Ok(())
}

/// Handle group details display
async fn handle_show_group(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    show_members: bool,
    show_metadata: bool,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);

    match chat_service.get_group(&group_id).await {
        Ok(Some(group)) => {
            ConsoleEffects::log_info(effect_system, &format!("📋 Group Details: {}", group.name))
                .await?;

            ConsoleEffects::log_info(effect_system, &format!("  ID: {}", group.id)).await?;

            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "  Created: {}",
                    group.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                ),
            )
            .await?;

            if !group.description.is_empty() {
                ConsoleEffects::log_info(
                    effect_system,
                    &format!("  Description: {}", group.description),
                )
                .await?;
            }

            if show_members {
                ConsoleEffects::log_info(
                    effect_system,
                    &format!("  Members ({}):", group.member_count()),
                )
                .await?;

                for member in &group.members {
                    ConsoleEffects::log_info(
                        effect_system,
                        &format!(
                            "    • {} ({:?}, joined {})",
                            member.display_name,
                            member.role,
                            member.joined_at.format("%Y-%m-%d")
                        ),
                    )
                    .await?;
                }
            }

            if show_metadata && !group.metadata.is_empty() {
                ConsoleEffects::log_info(effect_system, "  Metadata:").await?;
                for (key, value) in &group.metadata {
                    ConsoleEffects::log_info(effect_system, &format!("    {}: {}", key, value))
                        .await?;
                }
            }
        }
        Ok(None) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Group {} not found", group_id))
                .await?;
            anyhow::bail!("Group not found");
        }
        Err(e) => {
            ConsoleEffects::log_error(
                effect_system,
                &format!("✗ Failed to get group details: {}", e),
            )
            .await?;
            anyhow::bail!("Group retrieval failed: {}", e);
        }
    }

    Ok(())
}

/// Handle member invitation command
async fn handle_invite_member(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    authority_id: &AuthorityId,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let inviter_id = ctx.authority_id().clone();

    match chat_service
        .add_member(&group_id, inviter_id, authority_id.clone())
        .await
    {
        Ok(()) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "✓ Successfully invited {} to group {}",
                    authority_id, group_id
                ),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to invite member: {}", e))
                .await?;
            anyhow::bail!("Member invitation failed: {}", e);
        }
    }

    Ok(())
}

/// Handle leave group command
async fn handle_leave_group(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let member_id = ctx.authority_id().clone();

    match chat_service
        .remove_member(&group_id, member_id.clone(), member_id)
        .await
    {
        Ok(()) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!("✓ Successfully left group {}", group_id),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to leave group: {}", e))
                .await?;
            anyhow::bail!("Leave group failed: {}", e);
        }
    }

    Ok(())
}

/// Handle member removal command
async fn handle_remove_member(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    member_id: &AuthorityId,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let admin_id = ctx.authority_id().clone();

    match chat_service
        .remove_member(&group_id, admin_id, member_id.clone())
        .await
    {
        Ok(()) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!(
                    "✓ Successfully removed {} from group {}",
                    member_id, group_id
                ),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to remove member: {}", e))
                .await?;
            anyhow::bail!("Member removal failed: {}", e);
        }
    }

    Ok(())
}

/// Handle group update command (placeholder)
async fn handle_update_group(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    name: &Option<String>,
    description: &Option<String>,
    metadata: &[String],
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let mut metadata_map = HashMap::new();
    for entry in metadata {
        if let Some((key, value)) = entry.split_once('=') {
            metadata_map.insert(key.to_string(), value.to_string());
        }
    }

    let updated = chat_service
        .update_group_details(
            &group_id,
            ctx.authority_id().clone(),
            name.clone(),
            description.clone(),
            if metadata_map.is_empty() {
                None
            } else {
                Some(metadata_map)
            },
        )
        .await?;

    ConsoleEffects::log_info(
        effect_system,
        &format!("🔧 Updated group {} ({})", updated.name, updated.id),
    )
    .await?;

    Ok(())
}

/// Handle message search command (placeholder)
async fn handle_search_messages(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    query: &str,
    group_id: Option<&Uuid>,
    limit: usize,
    sender: Option<&AuthorityId>,
) -> Result<()> {
    let mut results = Vec::new();

    if let Some(group) = group_id {
        let group_id = ChatGroupId::from_uuid(*group);
        results = chat_service
            .search_messages(&group_id, query, limit, sender)
            .await?;
    } else {
        // Search across all of the caller's groups
        let groups = chat_service.list_user_groups(&ctx.authority_id()).await?;
        for group in groups {
            let mut found = chat_service
                .search_messages(
                    &group.id,
                    query,
                    limit.saturating_sub(results.len()),
                    sender,
                )
                .await?;
            results.append(&mut found);
            if results.len() >= limit {
                break;
            }
        }
    }

    if results.is_empty() {
        ConsoleEffects::log_info(effect_system, "🔍 No messages found").await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        effect_system,
        &format!("🔍 Found {} messages matching '{}'", results.len(), query),
    )
    .await?;

    for msg in results {
        let ts = msg.timestamp.format("%Y-%m-%d %H:%M:%S");
        ConsoleEffects::log_info(
            effect_system,
            &format!("[{}][{}] {}", ts, msg.group_id, msg.content),
        )
        .await?;
    }

    Ok(())
}

/// Handle message editing command (placeholder)
async fn handle_edit_message(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    message_id: &Uuid,
    content: &str,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let message_id = ChatMessageId::from_uuid(*message_id);

    match chat_service
        .edit_message(&group_id, ctx.authority_id().clone(), &message_id, content)
        .await
    {
        Ok(updated) => {
            ConsoleEffects::log_info(
                effect_system,
                &format!("✏️ Message {} updated at {}", updated.id, updated.timestamp),
            )
            .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to edit message: {}", e))
                .await?;
            return Err(e.into());
        }
    }

    Ok(())
}

/// Handle message deletion command (placeholder)
async fn handle_delete_message(
    ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    message_id: &Uuid,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let message_id = ChatMessageId::from_uuid(*message_id);

    match chat_service
        .delete_message(&group_id, ctx.authority_id().clone(), &message_id)
        .await
    {
        Ok(()) => {
            ConsoleEffects::log_info(effect_system, &format!("🗑️ Message {} deleted", message_id))
                .await?;
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("✗ Failed to delete message: {}", e))
                .await?;
            return Err(e.into());
        }
    }

    Ok(())
}

/// Handle history export command (placeholder)
async fn handle_export_history(
    _ctx: &EffectContext,
    chat_service: &ChatService<AuraEffectSystem>,
    effect_system: &AuraEffectSystem,
    group_id: &Uuid,
    output: &str,
    format: &str,
    include_system: bool,
) -> Result<()> {
    let group_id = ChatGroupId::from_uuid(*group_id);
    let history = chat_service.get_history(&group_id, None, None).await?;

    let filtered: Vec<_> = history
        .into_iter()
        .filter(|m| include_system || m.message_type != aura_chat::types::MessageType::System)
        .collect();

    let mut file = File::create(output)?;

    match format.to_lowercase().as_str() {
        "json" => {
            let serialized = serde_json::to_vec_pretty(&filtered)?;
            file.write_all(&serialized)?;
        }
        "text" => {
            for msg in filtered {
                let line = format!(
                    "[{}][{}] {}\n",
                    msg.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    msg.sender_id,
                    msg.content
                );
                file.write_all(line.as_bytes())?;
            }
        }
        "csv" => {
            file.write_all(b"timestamp,sender,group,content\n")?;
            for msg in filtered {
                let line = format!(
                    "{},{},{},\"{}\"\n",
                    msg.timestamp.to_rfc3339(),
                    msg.sender_id,
                    msg.group_id,
                    msg.content.replace('"', "\"\"")
                );
                file.write_all(line.as_bytes())?;
            }
        }
        other => {
            return Err(AuraError::invalid(format!("Unsupported export format {}", other)).into());
        }
    }

    ConsoleEffects::log_info(effect_system, &format!("📤 Exported history to {}", output)).await?;

    Ok(())
}

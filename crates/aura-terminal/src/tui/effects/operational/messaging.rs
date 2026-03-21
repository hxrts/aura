//! Messaging command handlers - TUI Operational Layer
//!
//! This module provides TUI-specific messaging operation handling.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::ui::workflows::messaging` (portable)
//! - **TUI Integration**: This module (operational layer)
//!
//! Handlers for SendMessage, CreateChannel, SendDirectMessage, StartDirectChat, SendAction, InviteUser.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;
use tracing::error;

use super::types::{OpError, OpFailureCode, OpResponse, OpResult};
use super::EffectCommand;
use crate::tui::tasks::UiTaskOwner;

// Re-export workflow functions for convenience
// Note: Primary functions accept typed ChannelId directly (typesafe API)
// TUI uses *_by_name variants for string-based user input
pub use aura_app::ui::workflows::messaging::{
    close_channel_by_name, create_channel_with_authoritative_binding, join_channel_by_name,
    leave_channel_by_name, send_action_by_name, send_direct_message,
    send_direct_message_to_authority, send_message, send_message_by_name, set_topic_by_name,
    start_direct_chat,
};

fn compact_send_error(error: &aura_core::AuraError) -> String {
    let raw = error.to_string();
    raw.rsplit(": ")
        .next()
        .map(str::trim)
        .filter(|tail| !tail.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(raw)
}

async fn resolve_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
    channel_ref: &str,
) -> Option<aura_core::types::identifiers::ChannelId> {
    let core = app_core.read().await;
    let chat_state = core.read(&*CHAT_SIGNAL).await.ok()?;

    if let Some(channel) = chat_state
        .all_channels()
        .find(|channel| channel.id.to_string() == channel_ref || channel.name == channel_ref)
    {
        return Some(channel.id);
    }

    channel_ref
        .parse::<aura_core::types::identifiers::ChannelId>()
        .ok()
}

/// Handle messaging commands
///
/// This is now a thin wrapper around workflow functions.
/// Business logic lives in aura_app::ui::workflows::messaging.
pub async fn handle_messaging(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
    tasks: &Arc<UiTaskOwner>,
) -> Option<OpResult> {
    match command {
        EffectCommand::CreateChannel {
            name,
            topic,
            members,
            threshold_k,
        } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            match create_channel_with_authoritative_binding(
                app_core,
                name,
                topic.clone(),
                members,
                *threshold_k,
                timestamp,
            )
            .await
            {
                Ok(created_channel) => Some(Ok(OpResponse::ChannelCreated {
                    channel_id: created_channel.channel_id.to_string(),
                    context_id: created_channel
                        .context_id
                        .map(|context_id| context_id.to_string()),
                })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::CreateChannel,
                    format!("Failed to create channel: {e}"),
                ))),
            }
        }

        EffectCommand::SendMessage { channel, content } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            let result = if let Some(channel_id) = resolve_channel_id(app_core, channel).await {
                send_message(app_core, channel_id, content, timestamp).await
            } else {
                send_message_by_name(app_core, channel, content, timestamp).await
            };
            match result {
                Ok(message_id) => Some(Ok(OpResponse::ChannelMessageSent { message_id })),
                Err(e) => {
                    let compact = compact_send_error(&e);
                    error!(
                        channel = %channel,
                        error = %e,
                        "tui send_message failed"
                    );
                    Some(Err(OpError::typed(
                        OpFailureCode::SendMessage,
                        format!("Failed to send message: {compact}"),
                    )))
                }
            }
        }

        EffectCommand::SendDirectMessage { target, content } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            let result = if let Ok(authority_id) =
                target.parse::<aura_core::types::identifiers::AuthorityId>()
            {
                send_direct_message_to_authority(app_core, authority_id, content, timestamp)
                    .await
                    .map(|channel_id| channel_id.to_string())
            } else {
                send_direct_message(app_core, target, content, timestamp).await
            };
            match result {
                Ok(dm_channel_id) => Some(Ok(OpResponse::DirectMessageSent {
                    channel_id: dm_channel_id,
                })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::SendDirectMessage,
                    format!("Failed to send message: {e}"),
                ))),
            }
        }

        EffectCommand::StartDirectChat { contact_id } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            match start_direct_chat(app_core, contact_id, timestamp).await {
                Ok(dm_channel_id) => Some(Ok(OpResponse::DirectMessageSent {
                    channel_id: dm_channel_id,
                })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::StartDirectChat,
                    format!("Failed to start chat: {e}"),
                ))),
            }
        }

        EffectCommand::SetTopic { channel, text } => {
            // Use set_topic_by_name for string-based channel input from TUI
            match set_topic_by_name(
                app_core,
                channel,
                text,
                super::time::current_time_ms(app_core).await,
            )
            .await
            {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::SetTopic,
                    format!("Failed to set topic: {e}"),
                ))),
            }
        }

        EffectCommand::SendAction { channel, action } => {
            // IRC-style /me action - use workflow
            let timestamp = super::time::current_time_ms(app_core).await;
            // Use send_action_by_name for string-based channel input from TUI
            match send_action_by_name(app_core, channel, action, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::ActionSent { message_id })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::SendAction,
                    format!("Failed to send action: {e}"),
                ))),
            }
        }

        EffectCommand::InviteUser {
            target,
            channel,
            context_id,
            operation_instance_id,
        } => {
            let parsed_context_id = context_id
                .as_deref()
                .and_then(|context_id| context_id.parse::<aura_core::ContextId>().ok());
            let app_core = Arc::clone(app_core);
            let tasks = Arc::clone(tasks);
            let target = target.clone();
            let channel = channel.clone();
            let operation_instance_id = operation_instance_id.clone();
            tasks.spawn(async move {
                if let Err(error) =
                    aura_app::ui::workflows::messaging::invite_user_to_channel_with_context(
                        &app_core,
                        &target,
                        &channel,
                        parsed_context_id,
                        operation_instance_id,
                        None,
                        None,
                    )
                    .await
                {
                    error!(target = %target, channel = %channel, error = %error, "tui invite_user_to_channel failed");
                }
            });
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::JoinChannel { channel } => {
            // Use join_channel_by_name for string-based channel input from TUI
            match join_channel_by_name(app_core, channel).await {
                Ok(()) => Some(Ok(OpResponse::ChannelJoined {
                    channel_id: channel.clone(),
                })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::JoinChannel,
                    format!("Failed to join channel: {e}"),
                ))),
            }
        }

        EffectCommand::LeaveChannel { channel } => {
            // Use leave_channel_by_name for string-based channel input from TUI
            match leave_channel_by_name(app_core, channel).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::LeaveChannel,
                    format!("Failed to leave channel: {e}"),
                ))),
            }
        }

        EffectCommand::CloseChannel { channel } => {
            // Use close_channel_by_name for string-based channel input from TUI
            match close_channel_by_name(
                app_core,
                channel,
                super::time::current_time_ms(app_core).await,
            )
            .await
            {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::CloseChannel,
                    format!("Failed to close channel: {e}"),
                ))),
            }
        }

        EffectCommand::RetryMessage {
            message_id: _,
            channel,
            content,
        } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            let result = if let Some(channel_id) = resolve_channel_id(app_core, channel).await {
                send_message(app_core, channel_id, content, timestamp).await
            } else {
                send_message_by_name(app_core, channel, content, timestamp).await
            };
            match result {
                Ok(message_id) => Some(Ok(OpResponse::RetrySent { message_id })),
                Err(e) => Some(Err(OpError::typed(
                    OpFailureCode::RetryMessage,
                    format!("Failed to retry message: {e}"),
                ))),
            }
        }

        _ => None,
    }
}

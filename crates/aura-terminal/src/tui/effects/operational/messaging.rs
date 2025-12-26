//! Messaging command handlers - TUI Operational Layer
//!
//! This module provides TUI-specific messaging operation handling.
//! Business logic has been moved to `aura_app::workflows::messaging`.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::workflows::messaging` (portable)
//! - **TUI Integration**: This module (operational layer)
//!
//! Handlers for SendMessage, CreateChannel, SendDirectMessage, StartDirectChat, SendAction, InviteUser.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflow functions for convenience
pub use aura_app::workflows::messaging::{
    close_channel, create_channel, invite_user_to_channel, join_channel, leave_channel,
    send_action, send_direct_message, send_message, set_topic, start_direct_chat,
};

/// Handle messaging commands
///
/// This is now a thin wrapper around workflow functions.
/// Business logic lives in aura_app::workflows::messaging.
pub async fn handle_messaging(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::CreateChannel {
            name,
            topic,
            members,
            threshold_k,
        } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            match create_channel(app_core, name, topic.clone(), members, *threshold_k, timestamp)
                .await
            {
                Ok(channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Channel created: {}",
                    channel_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to create channel: {}",
                    e
                )))),
            }
        }

        EffectCommand::SendMessage { channel, content } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            match send_message(app_core, channel, content, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!(
                    "Message sent: {}",
                    message_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send message: {}",
                    e
                )))),
            }
        }

        EffectCommand::SendDirectMessage { target, content } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            match send_direct_message(app_core, target, content, timestamp).await {
                Ok(dm_channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Message sent to DM channel: {}",
                    dm_channel_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send message: {}",
                    e
                )))),
            }
        }

        EffectCommand::StartDirectChat { contact_id } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            match start_direct_chat(app_core, contact_id, timestamp).await {
                Ok(dm_channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Started DM chat: {}",
                    dm_channel_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to start chat: {}",
                    e
                )))),
            }
        }

        EffectCommand::SetTopic { channel, text } => {
            match set_topic(
                app_core,
                channel,
                text,
                super::time::current_time_ms(app_core).await,
            )
            .await
            {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to set topic: {}",
                    e
                )))),
            }
        }

        EffectCommand::SendAction { channel, action } => {
            // IRC-style /me action - use workflow
            let timestamp = super::time::current_time_ms(app_core).await;
            match send_action(app_core, channel, action, timestamp).await {
                Ok(message_id) => {
                    Some(Ok(OpResponse::Data(format!("Action sent: {}", message_id))))
                }
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send action: {}",
                    e
                )))),
            }
        }

        EffectCommand::InviteUser { target } => {
            // Invite user to current channel - use workflow
            match invite_user_to_channel(app_core, target, None, None, None).await {
                Ok(invitation_id) => Some(Ok(OpResponse::Data(format!(
                    "Invitation sent: {}",
                    invitation_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to invite user: {}",
                    e
                )))),
            }
        }

        EffectCommand::JoinChannel { channel } => match join_channel(app_core, channel).await {
            Ok(()) => Some(Ok(OpResponse::Data(format!("Joined channel: {}", channel)))),
            Err(e) => Some(Err(super::types::OpError::Failed(format!(
                "Failed to join channel: {}",
                e
            )))),
        },

        EffectCommand::LeaveChannel { channel } => match leave_channel(app_core, channel).await {
            Ok(()) => Some(Ok(OpResponse::Ok)),
            Err(e) => Some(Err(super::types::OpError::Failed(format!(
                "Failed to leave channel: {}",
                e
            )))),
        },

        EffectCommand::CloseChannel { channel } => {
            match close_channel(
                app_core,
                channel,
                super::time::current_time_ms(app_core).await,
            )
            .await
            {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to close channel: {}",
                    e
                )))),
            }
        }

        EffectCommand::RetryMessage {
            message_id: _,
            channel,
            content,
        } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            match send_message(app_core, channel, content, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!(
                    "Message retried: {}",
                    message_id
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to retry message: {}",
                    e
                )))),
            }
        }

        _ => None,
    }
}

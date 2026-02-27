//! Messaging command handlers - TUI Operational Layer
//!
//! This module provides TUI-specific messaging operation handling.
//! Business logic has been moved to `aura_app::ui::workflows::messaging`.
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

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflow functions for convenience
// Note: Primary functions accept typed ChannelId directly (typesafe API)
// TUI uses *_by_name variants for string-based user input
pub use aura_app::ui::workflows::messaging::{
    close_channel_by_name, create_channel, invite_user_to_channel, join_channel_by_name,
    leave_channel_by_name, send_action_by_name, send_direct_message, send_message_by_name,
    set_topic_by_name, start_direct_chat,
};

/// Handle messaging commands
///
/// This is now a thin wrapper around workflow functions.
/// Business logic lives in aura_app::ui::workflows::messaging.
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
            match create_channel(
                app_core,
                name,
                topic.clone(),
                members,
                *threshold_k,
                timestamp,
            )
            .await
            {
                Ok(channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Channel created: {channel_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to create channel: {e}"
                )))),
            }
        }

        EffectCommand::SendMessage { channel, content } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            // Use send_message_by_name for string-based channel input from TUI
            match send_message_by_name(app_core, channel, content, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!("Message sent: {message_id}")))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send message: {e}"
                )))),
            }
        }

        EffectCommand::SendDirectMessage { target, content } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            match send_direct_message(app_core, target, content, timestamp).await {
                Ok(dm_channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Message sent to DM channel: {dm_channel_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send message: {e}"
                )))),
            }
        }

        EffectCommand::StartDirectChat { contact_id } => {
            // Use workflow for business logic
            let timestamp = super::time::current_time_ms(app_core).await;
            match start_direct_chat(app_core, contact_id, timestamp).await {
                Ok(dm_channel_id) => Some(Ok(OpResponse::Data(format!(
                    "Started DM chat: {dm_channel_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to start chat: {e}"
                )))),
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
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to set topic: {e}"
                )))),
            }
        }

        EffectCommand::SendAction { channel, action } => {
            // IRC-style /me action - use workflow
            let timestamp = super::time::current_time_ms(app_core).await;
            // Use send_action_by_name for string-based channel input from TUI
            match send_action_by_name(app_core, channel, action, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!("Action sent: {message_id}")))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to send action: {e}"
                )))),
            }
        }

        EffectCommand::InviteUser { target, channel } => {
            // Invite user to channel - use workflow
            match invite_user_to_channel(app_core, target, channel, None, None).await {
                Ok(invitation_id) => Some(Ok(OpResponse::Data(format!(
                    "Invitation sent: {invitation_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to invite user: {e}"
                )))),
            }
        }

        EffectCommand::JoinChannel { channel } => {
            // Use join_channel_by_name for string-based channel input from TUI
            match join_channel_by_name(app_core, channel).await {
                Ok(()) => Some(Ok(OpResponse::Data(format!("Joined channel: {channel}")))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to join channel: {e}"
                )))),
            }
        }

        EffectCommand::LeaveChannel { channel } => {
            // Use leave_channel_by_name for string-based channel input from TUI
            match leave_channel_by_name(app_core, channel).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to leave channel: {e}"
                )))),
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
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to close channel: {e}"
                )))),
            }
        }

        EffectCommand::RetryMessage {
            message_id: _,
            channel,
            content,
        } => {
            let timestamp = super::time::current_time_ms(app_core).await;
            // Use send_message_by_name for string-based channel input from TUI
            match send_message_by_name(app_core, channel, content, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!(
                    "Message retried: {message_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to retry message: {e}"
                )))),
            }
        }

        _ => None,
    }
}

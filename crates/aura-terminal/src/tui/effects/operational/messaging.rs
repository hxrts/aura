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
//! Handlers for SendDirectMessage, StartDirectChat, SendAction, InviteUser.

use std::sync::Arc;

use aura_app::AppCore;
use async_lock::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflow functions for convenience
pub use aura_app::workflows::messaging::{
    invite_user_to_channel, send_action, send_direct_message, start_direct_chat,
};

/// Get current time in milliseconds since Unix epoch
///
/// Used to provide timestamps to pure workflow functions.
fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Handle messaging commands
///
/// This is now a thin wrapper around workflow functions.
/// Business logic lives in aura_app::workflows::messaging.
pub async fn handle_messaging(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::SendDirectMessage { target, content } => {
            // Use workflow for business logic
            let timestamp = current_time_ms();
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
            let timestamp = current_time_ms();
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

        EffectCommand::SendAction { channel, action } => {
            // IRC-style /me action - use workflow
            let timestamp = current_time_ms();
            match send_action(app_core, channel, action, timestamp).await {
                Ok(message_id) => Some(Ok(OpResponse::Data(format!(
                    "Action sent: {}",
                    message_id
                )))),
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

        _ => None,
    }
}

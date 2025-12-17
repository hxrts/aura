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
pub use aura_app::workflows::messaging::{send_direct_message, start_direct_chat};

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
            match send_direct_message(app_core, target, content).await {
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
            match start_direct_chat(app_core, contact_id).await {
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

        EffectCommand::SendAction {
            channel: _,
            action: _,
        } => {
            // IRC-style /me action
            // TODO: Implement workflow for SendAction
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::InviteUser { target: _ } => {
            // TODO: Implement workflow for InviteUser
            Some(Ok(OpResponse::Ok))
        }

        _ => None,
    }
}

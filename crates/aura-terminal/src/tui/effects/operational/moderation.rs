//! Moderation command handlers
//!
//! Handlers for KickUser, BanUser, UnbanUser, MuteUser, UnmuteUser, PinMessage, UnpinMessage.
//!
//! This module delegates to portable workflows in `aura_app::workflows::moderation`.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

pub use aura_app::workflows::moderation::{
    ban_user, kick_user, mute_user, pin_message, unban_user, unmute_user, unpin_message,
};

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Handle moderation commands.
pub async fn handle_moderation(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::KickUser {
            channel: _,
            target,
            reason,
        } => {
            let ts = current_time_ms();
            match kick_user(app_core, target, reason.as_deref(), ts).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::BanUser { target, reason } => {
            let ts = current_time_ms();
            match ban_user(app_core, target, reason.as_deref(), ts).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::UnbanUser { target } => match unban_user(app_core, target).await {
            Ok(()) => Some(Ok(OpResponse::Ok)),
            Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
        },

        EffectCommand::MuteUser {
            target,
            duration_secs,
        } => {
            let ts = current_time_ms();
            match mute_user(app_core, target, *duration_secs, ts).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::UnmuteUser { target } => match unmute_user(app_core, target).await {
            Ok(()) => Some(Ok(OpResponse::Ok)),
            Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
        },

        EffectCommand::PinMessage { message_id } => match pin_message(app_core, message_id).await {
            Ok(()) => Some(Ok(OpResponse::Ok)),
            Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
        },

        EffectCommand::UnpinMessage { message_id } => {
            match unpin_message(app_core, message_id).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}

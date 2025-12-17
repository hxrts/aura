//! Settings command handlers
//!
//! Handlers for UpdateMfaPolicy, UpdateNickname, SetChannelMode.
//!
//! This module delegates to portable workflows in aura_app::workflows::settings
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use aura_app::AppCore;
use async_lock::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::settings::{set_channel_mode, update_mfa_policy, update_nickname};

/// Handle settings commands
pub async fn handle_settings(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::UpdateMfaPolicy { require_mfa } => {
            // Delegate to workflow
            match update_mfa_policy(app_core, *require_mfa).await {
                Ok(()) => Some(Ok(OpResponse::MfaPolicyUpdated {
                    require_mfa: *require_mfa,
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::UpdateNickname { name } => {
            // Delegate to workflow
            match update_nickname(app_core, name.clone()).await {
                Ok(()) => Some(Ok(OpResponse::NicknameUpdated { name: name.clone() })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::SetChannelMode { channel, flags } => {
            // Delegate to workflow
            match set_channel_mode(app_core, channel.clone(), flags.clone()).await {
                Ok(()) => Some(Ok(OpResponse::ChannelModeSet {
                    channel_id: channel.clone(),
                    flags: flags.clone(),
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}

//! Settings command handlers
//!
//! Handlers for UpdateMfaPolicy, UpdateNickname, SetChannelMode.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::settings
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
use aura_app::ui::workflows::ceremonies::{
    start_device_enrollment_ceremony, start_device_removal_ceremony,
};
pub use aura_app::ui::workflows::settings::update_threshold;
pub use aura_app::ui::workflows::settings::{set_channel_mode, update_mfa_policy, update_nickname};

/// Handle settings commands
pub async fn handle_settings(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::AddDevice {
            nickname_suggestion,
            invitee_authority_id,
        } => {
            match start_device_enrollment_ceremony(
                app_core,
                nickname_suggestion.clone(),
                invitee_authority_id.clone(),
            )
            .await
            {
                Ok(start) => Some(Ok(OpResponse::DeviceEnrollmentStarted {
                    ceremony_id: start.ceremony_id,
                    enrollment_code: start.enrollment_code,
                    pending_epoch: start.pending_epoch,
                    device_id: start.device_id.to_string(),
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::RemoveDevice { device_id } => {
            match start_device_removal_ceremony(app_core, device_id.clone()).await {
                Ok(ceremony_id) => Some(Ok(OpResponse::DeviceRemovalStarted { ceremony_id })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

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

        EffectCommand::UpdateThreshold {
            threshold_k,
            threshold_n,
        } => match update_threshold(app_core, *threshold_k, *threshold_n).await {
            Ok(()) => Some(Ok(OpResponse::Ok)),
            Err(e) => Some(Err(super::types::OpError::Failed(format!(
                "Failed to update threshold: {e}"
            )))),
        },

        _ => None,
    }
}

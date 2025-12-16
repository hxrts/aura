//! Settings command handlers
//!
//! Handlers for UpdateMfaPolicy, UpdateNickname, SetChannelMode.

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle settings commands
pub async fn handle_settings(command: &EffectCommand) -> Option<OpResult> {
    match command {
        EffectCommand::UpdateMfaPolicy { require_mfa } => {
            // Return the MFA policy update so IoContext can update its state
            Some(Ok(OpResponse::MfaPolicyUpdated {
                require_mfa: *require_mfa,
            }))
        }

        EffectCommand::UpdateNickname { name } => {
            // Return the nickname update so IoContext can update its state
            Some(Ok(OpResponse::NicknameUpdated { name: name.clone() }))
        }

        EffectCommand::SetChannelMode { channel, flags } => {
            // Return the channel mode info so IoContext can update local storage
            Some(Ok(OpResponse::ChannelModeSet {
                channel_id: channel.clone(),
                flags: flags.clone(),
            }))
        }

        _ => None,
    }
}

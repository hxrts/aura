//! Settings command handlers
//!
//! Handlers for UpdateMfaPolicy, UpdateNickname, SetChannelMode.

use std::sync::Arc;

use aura_app::signal_defs::{DeviceInfo, SettingsState, SETTINGS_SIGNAL};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle settings commands
pub async fn handle_settings(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::UpdateMfaPolicy { require_mfa } => {
            // Emit settings signal after update
            emit_settings_signal(app_core).await;

            Some(Ok(OpResponse::MfaPolicyUpdated {
                require_mfa: *require_mfa,
            }))
        }

        EffectCommand::UpdateNickname { name } => {
            // Emit settings signal after update
            emit_settings_signal(app_core).await;

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

/// Helper function to emit settings signal with current state
async fn emit_settings_signal(app_core: &Arc<RwLock<AppCore>>) {
    let core = app_core.read().await;

    // Get current settings from runtime
    // For now, we use placeholder values - these would be queried from runtime in production
    let display_name = String::new(); // TODO: Query from runtime
    let threshold_k = 0; // TODO: Query from runtime
    let threshold_n = 0; // TODO: Query from runtime
    let mfa_policy = "SensitiveOnly".to_string(); // TODO: Query from runtime
    let devices: Vec<DeviceInfo> = Vec::new(); // TODO: Query device list from runtime
    let contact_count = 0; // TODO: Query contact count from runtime

    let state = SettingsState {
        display_name,
        threshold_k,
        threshold_n,
        mfa_policy,
        devices,
        contact_count,
    };

    // Emit the signal
    let _ = core.emit(&*SETTINGS_SIGNAL, state).await;
}

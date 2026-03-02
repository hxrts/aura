//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::workflows::runtime::require_runtime;
use crate::workflows::signals::{emit_signal, read_signal};
use crate::workflows::state_helpers::with_recovery_state;
use crate::workflows::{channel_ref::ChannelRef, snapshot_policy::chat_snapshot};
use crate::{
    signal_defs::{DeviceInfo, SettingsState, SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME},
    thresholds::normalize_recovery_threshold,
    AppCore,
};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

async fn refresh_settings_signal_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (settings, devices, authority_id) = {
        let core = app_core.read().await;
        let authority_id = core
            .runtime()
            .map(|r| r.authority_id().to_string())
            .unwrap_or_default();
        match core.settings_snapshot().await {
            Some(snapshot) => (snapshot.0, snapshot.1, authority_id),
            None => return Ok(()),
        }
    };
    let mut state = read_signal(app_core, &*SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await?;
    state.nickname_suggestion = settings.nickname_suggestion.clone();
    state.mfa_policy = settings.mfa_policy;
    state.threshold_k = settings.threshold_k as u8;
    state.threshold_n = settings.threshold_n as u8;
    state.contact_count = settings.contact_count;
    state.devices = devices
        .into_iter()
        .map(|d| DeviceInfo {
            id: d.id,
            name: d.name,
            is_current: d.is_current,
            last_seen: d.last_seen,
        })
        .collect();
    state.authority_id = authority_id;
    state.authority_nickname = settings.nickname_suggestion;

    emit_signal(app_core, &*SETTINGS_SIGNAL, state, SETTINGS_SIGNAL_NAME).await
}

/// Refresh SETTINGS_SIGNAL from the current RuntimeBridge settings.
///
/// This is used at startup (to seed UI state) and after settings writes.
pub async fn refresh_settings_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    refresh_settings_signal_from_runtime(app_core).await
}

/// Update MFA policy
///
/// **What it does**: Updates MFA policy and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn update_mfa_policy(
    app_core: &Arc<RwLock<AppCore>>,
    require_mfa: bool,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let policy = if require_mfa {
        "AlwaysRequired"
    } else {
        "SensitiveOnly"
    };

    runtime
        .set_mfa_policy(policy)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update MFA policy: {e}")))?;

    refresh_settings_from_runtime(app_core).await?;
    Ok(())
}

/// Update nickname suggestion (what the user wants to be called)
///
/// **What it does**: Updates nickname suggestion and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn update_nickname(
    app_core: &Arc<RwLock<AppCore>>,
    name: String,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .set_nickname_suggestion(&name)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update nickname: {e}")))?;

    refresh_settings_from_runtime(app_core).await?;
    Ok(())
}

/// Set channel mode flags
///
/// **What it does**: Sets channel-specific mode flags
/// **Returns**: Unit result
/// **Signal pattern**: Read-only operation (no emission)
///
/// This operation updates local channel preferences (e.g., notifications).
/// The UI layer handles persistence to local storage.
pub async fn set_channel_mode(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: String,
    flags: String,
) -> Result<(), AuraError> {
    let normalized_channel = crate::workflows::chat_commands::normalize_channel_name(&channel_id);
    let chat = chat_snapshot(app_core).await;
    let resolved_channel = {
        let parsed = ChannelRef::parse(&normalized_channel).to_channel_id();
        chat.all_channels()
            .find(|entry| {
                entry.id == parsed || entry.name.eq_ignore_ascii_case(&normalized_channel)
            })
            .map(|entry| entry.id)
            .unwrap_or(parsed)
    };

    let context_hint = chat
        .channel(&resolved_channel)
        .and_then(|channel| channel.context_id);

    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();

    let mut target_home_id = if homes.has_home(&resolved_channel) {
        Some(resolved_channel)
    } else {
        context_hint
            .and_then(|context_id| {
                homes
                    .iter()
                    .filter(|(_, home)| home.context_id == Some(context_id))
                    .max_by_key(|(_, home)| {
                        (
                            u8::from(home.is_admin()),
                            u8::from(!home.residents.is_empty()),
                            home.resident_count,
                        )
                    })
                    .map(|(home_id, _)| *home_id)
            })
            .or_else(|| homes.current_home_id().copied())
            .or_else(|| {
                homes
                    .iter()
                    .filter(|(_, home)| home.is_admin())
                    .max_by_key(|(_, home)| {
                        (u8::from(!home.residents.is_empty()), home.resident_count)
                    })
                    .map(|(home_id, _)| *home_id)
            })
    };

    // Materialize a placeholder home when the channel context exists but
    // local HomesState has not yet converged for that context.
    if target_home_id.is_none() {
        if let Some(context_id) = context_hint {
            let owner = core
                .runtime()
                .map(|runtime| runtime.authority_id())
                .or_else(|| core.authority().copied())
                .unwrap_or_else(|| aura_core::identifiers::AuthorityId::new_from_entropy([0; 32]));

            let mut placeholder = crate::views::home::HomeState::new(
                resolved_channel,
                Some(normalized_channel.clone()),
                owner,
                0,
                context_id,
            );
            placeholder.my_role = crate::views::home::ResidentRole::Resident;
            placeholder.residents.clear();
            placeholder.resident_count = 0;
            placeholder.online_count = 0;
            homes.add_home_with_auto_select(placeholder);
            target_home_id = Some(resolved_channel);
        }
    }

    let Some(home_id) = target_home_id else {
        return Err(AuraError::permission_denied(format!(
            "Set channel mode requires a home context (channel: {channel_id})"
        )));
    };

    let home = homes
        .home_mut(&home_id)
        .ok_or_else(|| {
            AuraError::permission_denied("Set channel mode requires a valid home context")
        })?;
    if !home.is_admin() {
        return Err(AuraError::permission_denied(
            "Only stewards can set channel mode",
        ));
    }

    home.mode_flags = Some(flags);
    core.views_mut().set_homes(homes);

    Ok(())
}

/// Update guardian recovery threshold configuration.
///
/// This updates both:
/// - `RECOVERY_SIGNAL` threshold (used by recovery flows)
/// - `SETTINGS_SIGNAL` threshold fields (used by settings UI)
pub async fn update_threshold(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: u8,
    threshold_n: u8,
) -> Result<(), AuraError> {
    if threshold_n == 0 {
        return Err(AuraError::invalid("Threshold N must be greater than 0"));
    }

    let guardian_count = {
        let core = app_core.read().await;
        core.snapshot().recovery.guardian_count() as u8
    };

    if guardian_count == 0 {
        return Err(AuraError::invalid(
            "No guardians configured. Add guardians before setting a threshold.",
        ));
    }

    if threshold_n != guardian_count {
        return Err(AuraError::invalid(format!(
            "Threshold N ({threshold_n}) must match guardian count ({guardian_count})"
        )));
    }

    let normalized_k = normalize_recovery_threshold(threshold_k, threshold_n);

    with_recovery_state(app_core, |state| {
        state.set_threshold(normalized_k as u32);
    })
    .await?;

    let mut state = read_signal(app_core, &*SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await?;
    state.threshold_k = normalized_k;
    state.threshold_n = threshold_n;
    emit_signal(app_core, &*SETTINGS_SIGNAL, state, SETTINGS_SIGNAL_NAME).await?;

    Ok(())
}

/// Get current settings state
///
/// **What it does**: Reads settings from SETTINGS_SIGNAL
/// **Returns**: Current settings state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_settings(app_core: &Arc<RwLock<AppCore>>) -> Result<SettingsState, AuraError> {
    read_signal(app_core, &SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::{
        chat::{Channel, ChannelType, ChatState},
        home::HomeState,
    };
    use crate::AppConfig;
    use aura_core::{
        crypto::hash::hash,
        identifiers::{AuthorityId, ChannelId, ContextId},
    };

    #[tokio::test]
    async fn test_get_settings_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Workflows assume reactive signals are initialized.
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let settings = get_settings(&app_core).await.unwrap();
        assert_eq!(settings.threshold_k, 0);
        assert_eq!(settings.threshold_n, 0);
    }

    #[tokio::test]
    async fn test_update_mfa_policy_without_runtime() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Without a runtime bridge, updating MFA policy should fail
        let result = update_mfa_policy(&app_core, true).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Runtime bridge not available"));
    }

    #[tokio::test]
    async fn test_set_channel_mode_normalizes_hash_prefix() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let channel_id = crate::workflows::chat_commands::normalize_channel_name("#general");
        let channel_id =
            crate::workflows::channel_ref::ChannelRef::parse(&channel_id).to_channel_id();
        let creator = AuthorityId::new_from_entropy([9u8; 32]);
        let context = ContextId::new_from_entropy([7u8; 32]);
        let home = HomeState::new(channel_id, Some("general".to_string()), creator, 0, context);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            let _ = homes.add_home(home);
            homes.select_home(Some(channel_id));
            core.views_mut().set_homes(homes);
        }

        set_channel_mode(&app_core, "#general".to_string(), "+m".to_string())
            .await
            .expect("mode should be set for #general");

        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let home = homes.home_state(&channel_id).expect("home exists");
        assert_eq!(home.mode_flags.as_deref(), Some("+m"));
    }

    #[tokio::test]
    async fn test_set_channel_mode_falls_back_to_current_admin_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let creator = AuthorityId::new_from_entropy([5u8; 32]);
        let home_context = ContextId::new_from_entropy([6u8; 32]);
        let current_home_id = ChannelId::from_bytes(hash(b"settings-current-home"));
        let target_channel_id = ChannelId::from_bytes(hash(b"settings-target-channel"));
        let target_channel_name = "slash-lab".to_string();

        {
            let mut core = app_core.write().await;

            let home = HomeState::new(
                current_home_id,
                Some("admin-home".to_string()),
                creator,
                0,
                home_context,
            );
            let mut homes = core.views().get_homes();
            let _ = homes.add_home(home);
            homes.select_home(Some(current_home_id));
            core.views_mut().set_homes(homes);

            let mut chat = ChatState::new();
            chat.upsert_channel(Channel {
                id: target_channel_id,
                context_id: None,
                name: target_channel_name.clone(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![creator],
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
            core.views_mut().set_chat(chat);
        }

        set_channel_mode(&app_core, target_channel_name, "+m".to_string())
            .await
            .expect("mode should fall back to current admin home");

        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let home = homes
            .home_state(&current_home_id)
            .expect("current home should still exist");
        assert_eq!(home.mode_flags.as_deref(), Some("+m"));
    }
}

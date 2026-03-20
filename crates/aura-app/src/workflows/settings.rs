//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::error::WorkflowError;
use crate::workflows::runtime::require_runtime;
use crate::workflows::signals::{emit_signal, read_signal};
use crate::{
    signal_defs::{
        AuthorityInfo, DeviceInfo, SettingsState, HOMES_SIGNAL, HOMES_SIGNAL_NAME, RECOVERY_SIGNAL,
        RECOVERY_SIGNAL_NAME, SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME,
    },
    thresholds::normalize_recovery_threshold,
    views::{HomesState, RecoveryState},
    AppCore,
};
use async_lock::RwLock;
use aura_core::types::identifiers::ChannelId;
use aura_core::AuraError;
use std::sync::Arc;

// OWNERSHIP: authoritative-source
async fn refresh_settings_signal_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (settings, devices, authorities, authority_id) = {
        let core = app_core.read().await;
        let authority_id = core
            .runtime()
            .ok_or_else(|| AuraError::not_found("settings runtime missing"))?
            .authority_id()
            .to_string();
        match core.settings_snapshot().await {
            Ok(Some(snapshot)) => (snapshot.0, snapshot.1, snapshot.2, authority_id),
            Ok(None) => return Ok(()),
            Err(error) => {
                return Err(AuraError::from(super::error::runtime_call(
                    "refresh settings snapshot",
                    error,
                )));
            }
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
    state.authorities = authorities
        .into_iter()
        .map(|authority| -> Result<AuthorityInfo, AuraError> {
            Ok(AuthorityInfo {
                id: authority.id,
                nickname_suggestion: authority.nickname_suggestion.ok_or_else(|| {
                    AuraError::not_found(format!(
                        "authority {} has no nickname suggestion",
                        authority.id
                    ))
                })?,
                is_current: authority.is_current,
            })
        })
        .collect::<Result<Vec<_>, AuraError>>()?;

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

async fn homes_state_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<HomesState, AuraError> {
    read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await
}

async fn emit_homes_state_observed(
    app_core: &Arc<RwLock<AppCore>>,
    homes: HomesState,
) -> Result<(), AuraError> {
    {
        let mut core = app_core.write().await;
        // OWNERSHIP: observed-display-update
        core.views_mut().set_homes(homes.clone());
    }
    emit_signal(app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME).await
}

async fn emit_recovery_state_observed(
    app_core: &Arc<RwLock<AppCore>>,
    recovery: RecoveryState,
) -> Result<(), AuraError> {
    {
        let mut core = app_core.write().await;
        // OWNERSHIP: observed-display-update
        core.views_mut().set_recovery(recovery.clone());
    }
    emit_signal(app_core, &*RECOVERY_SIGNAL, recovery, RECOVERY_SIGNAL_NAME).await
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
        .map_err(|e| super::error::runtime_call("update MFA policy", e))?;

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
        .map_err(|e| super::error::runtime_call("update nickname", e))?;

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
    let runtime = require_runtime(app_core).await?;
    let normalized_channel = crate::workflows::chat_commands::normalize_channel_name(&channel_id);
    let resolved_channel = {
        match ChannelSelector::parse(&normalized_channel)? {
            ChannelSelector::Id(channel_id) => channel_id,
            ChannelSelector::Name(channel_name) => {
                let resolved = runtime
                    .resolve_authoritative_channel_ids_by_name(&channel_name)
                    .await
                    .map_err(|e| {
                        super::error::runtime_call("resolve channel for mode update", e)
                    })?;
                match resolved.as_slice() {
                    [] => return Err(AuraError::not_found(channel_name.clone())),
                    [channel_id] => *channel_id,
                    _ => {
                        return Err(AuraError::invalid(format!(
                            "Ambiguous channel name for mode update: {channel_name}"
                        )));
                    }
                }
            }
        }
    };

    set_channel_mode_resolved(app_core, resolved_channel, flags).await
}

/// Set channel mode flags using a canonical channel ID.
pub async fn set_channel_mode_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    resolved_channel: ChannelId,
    flags: String,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let context_id = runtime
        .resolve_amp_channel_context(resolved_channel)
        .await
        .map_err(|e| super::error::runtime_call("resolve channel context for mode update", e))?
        .ok_or_else(|| {
            AuraError::from(WorkflowError::MissingAuthoritativeContext {
                channel: resolved_channel.to_string(),
            })
        })?;
    let mut homes = homes_state_signal_snapshot(app_core).await?;

    let target_home_id = if homes.has_home(&resolved_channel) {
        Some(resolved_channel)
    } else {
        homes
            .iter()
            .filter(|(_, home)| home.context_id == Some(context_id))
            .max_by_key(|(_, home)| home.member_count)
            .map(|(home_id, _)| *home_id)
    };

    if target_home_id.is_none() {
        return Err(AuraError::from(
            WorkflowError::MissingAuthoritativeHomeProjection {
                context: context_id.to_string(),
            },
        ));
    }

    let Some(home_id) = target_home_id else {
        return Err(AuraError::permission_denied(resolved_channel.to_string()));
    };

    let home = homes.home_mut(&home_id).ok_or_else(|| {
        AuraError::permission_denied("Set channel mode requires a valid home context")
    })?;
    home.mode_flags = Some(flags);

    emit_homes_state_observed(app_core, homes).await
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

    let mut recovery = read_signal(app_core, &*RECOVERY_SIGNAL, RECOVERY_SIGNAL_NAME).await?;
    let guardian_count = recovery.guardian_count() as u8;

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

    recovery.set_threshold(normalized_k as u32);
    emit_recovery_state_observed(app_core, recovery).await?;

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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use crate::signal_defs::register_app_signals;
    use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
    use crate::views::home::HomeState;
    use crate::workflows::signals::{emit_signal, read_signal};
    use crate::AppConfig;
    use aura_core::{crypto::hash::hash, AuthorityId, ChannelId, ContextId};

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
        let authority_id = AuthorityId::new_from_entropy([8u8; 32]);
        let runtime = Arc::new(OfflineRuntimeBridge::new(authority_id));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }

        let channel_id = crate::workflows::chat_commands::normalize_channel_name("#general");
        let channel_id =
            crate::workflows::channel_ref::ChannelRef::parse(&channel_id).to_channel_id();
        let creator = AuthorityId::new_from_entropy([9u8; 32]);
        let context = ContextId::new_from_entropy([7u8; 32]);
        let home = HomeState::new(channel_id, Some("general".to_string()), creator, 0, context);
        let mut homes = HomesState::default();
        let _ = homes.add_home(home);
        homes.select_home(Some(channel_id));
        emit_signal(&app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        runtime.set_authoritative_channel_name_matches("general", vec![channel_id]);
        runtime.set_amp_channel_context(channel_id, context);

        set_channel_mode(&app_core, "#general".to_string(), "+m".to_string())
            .await
            .expect("mode should be set for #general");

        let homes = read_signal(&app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        let home = homes.home_state(&channel_id).expect("home exists");
        assert_eq!(home.mode_flags.as_deref(), Some("+m"));
    }

    #[tokio::test]
    async fn test_set_channel_mode_rejects_unscoped_channel_without_context() {
        let config = AppConfig::default();
        let creator = AuthorityId::new_from_entropy([5u8; 32]);
        let runtime = Arc::new(OfflineRuntimeBridge::new(creator));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }

        let home_context = ContextId::new_from_entropy([6u8; 32]);
        let current_home_id = ChannelId::from_bytes(hash(b"settings-current-home"));
        let target_channel_id = ChannelId::from_bytes(hash(b"settings-target-channel"));
        let target_channel_name = "slash-lab".to_string();

        let home = HomeState::new(
            current_home_id,
            Some("admin-home".to_string()),
            creator,
            0,
            home_context,
        );
        let mut homes = HomesState::default();
        let _ = homes.add_home(home);
        homes.select_home(Some(current_home_id));
        emit_signal(&app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        runtime
            .set_authoritative_channel_name_matches(&target_channel_name, vec![target_channel_id]);

        let error = set_channel_mode(&app_core, target_channel_name, "+m".to_string())
            .await
            .expect_err("mode update should fail without a channel-scoped home context");
        assert!(!error.to_string().is_empty());
    }
}

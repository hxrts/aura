//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::error::WorkflowError;
use crate::workflows::observed_projection::{
    try_update_homes_projection_observed, try_update_recovery_projection_observed,
};
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::workflows::signals::{emit_signal, read_signal};
use crate::{
    runtime_bridge::AuthoritativeChannelBinding,
    signal_defs::{
        AuthorityInfo, DeviceInfo, SettingsState, SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME,
    },
    thresholds::normalize_recovery_threshold,
    AppCore,
};
use async_lock::RwLock;
use aura_core::types::identifiers::ChannelId;
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

const SETTINGS_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

// OWNERSHIP: authoritative-source
async fn refresh_settings_signal_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (runtime, authority_id) = {
        let core = app_core.read().await;
        let runtime = core
            .runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?;
        let authority_id = runtime.authority_id().to_string();
        (runtime, authority_id)
    };
    let settings = timeout_runtime_call(
        &runtime,
        "refresh_settings_from_runtime",
        "try_get_settings",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.try_get_settings(),
    )
    .await?
    .map_err(|error| super::error::native_runtime_call("refresh settings", error))?;
    let devices = timeout_runtime_call(
        &runtime,
        "refresh_settings_from_runtime",
        "try_list_devices",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.try_list_devices(),
    )
    .await?
    .map_err(|error| super::error::native_runtime_call("list devices", error))?;
    let authorities = timeout_runtime_call(
        &runtime,
        "refresh_settings_from_runtime",
        "try_list_authorities",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.try_list_authorities(),
    )
    .await?
    .map_err(|error| super::error::native_runtime_call("list authorities", error))?;
    let pending_signing_requests = timeout_runtime_call(
        &runtime,
        "refresh_settings_from_runtime",
        "try_list_pending_signing_requests",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.try_list_pending_signing_requests(),
    )
    .await?
    .map_err(|error| super::error::native_runtime_call("list pending signing requests", error))?;
    let mut state = read_signal(app_core, &*SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await?;
    state.nickname_suggestion = settings.nickname_suggestion.clone();
    state.mfa_policy = settings.mfa_policy;
    state.signing_consent = settings.signing_consent;
    state.pending_signing_requests = pending_signing_requests;
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
                    AuraError::from(WorkflowError::Precondition(
                        "authority has no nickname suggestion",
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

    timeout_runtime_call(
        &runtime,
        "update_mfa_policy",
        "set_mfa_policy",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.set_mfa_policy(policy),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("update MFA policy", e))?;

    refresh_settings_from_runtime(app_core).await?;
    Ok(())
}

/// Set this device's consent policy for co-signing another device's quorum
/// request (device-local; never replicated), then refresh SETTINGS_SIGNAL.
pub async fn update_device_signing_consent(
    app_core: &Arc<RwLock<AppCore>>,
    consent: crate::runtime_bridge::DeviceSigningConsent,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "update_device_signing_consent",
        "set_device_signing_consent",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.set_device_signing_consent(consent),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("update device signing consent", e))?;
    refresh_settings_from_runtime(app_core).await
}

/// Approve or decline a quorum signing request from another device of this
/// account that waits for the user here, then refresh SETTINGS_SIGNAL.
pub async fn decide_pending_signing_request(
    app_core: &Arc<RwLock<AppCore>>,
    request_id: &str,
    approve: bool,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "decide_pending_signing_request",
        "decide_pending_signing_request",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.decide_pending_signing_request(request_id, approve),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("decide pending signing request", e))?;
    refresh_settings_from_runtime(app_core).await
}

/// Set the receive allowance granted to `peer` in `context`
///
/// **What it does**: Commits a per-peer flow allowance override fact
/// (docs/111 §3.1); every device of this authority enforces it from the
/// next window epoch.
/// **Returns**: Unit result
pub async fn set_peer_flow_allowance(
    app_core: &Arc<RwLock<AppCore>>,
    context: aura_core::types::identifiers::ContextId,
    peer: aura_core::types::identifiers::AuthorityId,
    window: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "set_peer_flow_allowance",
        "set_peer_flow_allowance",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.set_peer_flow_allowance(context, peer, window),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("set peer flow allowance", e))?;
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

    timeout_runtime_call(
        &runtime,
        "update_nickname",
        "set_nickname_suggestion",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.set_nickname_suggestion(&name),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("update nickname", e))?;

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
    match ChannelSelector::parse(&normalized_channel)? {
        ChannelSelector::Id(channel_id) => {
            set_channel_mode_resolved(app_core, channel_id, flags).await
        }
        ChannelSelector::Name(channel_name) => {
            let resolved = timeout_runtime_call(
                &runtime,
                "set_channel_mode",
                "identify_materialized_channel_bindings_by_name",
                SETTINGS_RUNTIME_TIMEOUT,
                || runtime.identify_materialized_channel_bindings_by_name(&channel_name),
            )
            .await?
            .map_err(|e| super::error::native_runtime_call("resolve channel for mode update", e))?;
            let binding = match resolved.as_slice() {
                [] => return Err(AuraError::not_found(channel_name.clone())),
                [binding] => *binding,
                _ => {
                    return Err(AuraError::invalid(format!(
                        "Ambiguous channel name for mode update: {channel_name}"
                    )));
                }
            };
            set_channel_mode_bound(app_core, binding, flags).await
        }
    }
}

/// Channel modes are channel management: only a moderator of the home may
/// set them (Task 11).
fn require_mode_authority(home: &crate::views::home::HomeState) -> Result<(), AuraError> {
    if home.is_admin() {
        Ok(())
    } else {
        Err(AuraError::permission_denied(
            "Only moderators can change channel mode",
        ))
    }
}

/// Set channel mode flags using a canonical channel ID.
pub async fn set_channel_mode_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    resolved_channel: ChannelId,
    flags: String,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let context_id = timeout_runtime_call(
        &runtime,
        "set_channel_mode_resolved",
        "resolve_amp_channel_context",
        SETTINGS_RUNTIME_TIMEOUT,
        || runtime.resolve_amp_channel_context(resolved_channel),
    )
    .await?
    .map_err(|e| super::error::native_runtime_call("resolve channel context for mode update", e))?
    .ok_or_else(|| {
        AuraError::from(WorkflowError::MissingAuthoritativeContext {
            channel: resolved_channel.to_string(),
        })
    })?;
    try_update_homes_projection_observed(app_core, |homes| {
        let target_home_id = if homes.has_home(&resolved_channel) {
            Some(resolved_channel)
        } else {
            homes
                .iter()
                .filter(|(_, home)| home.context_id == Some(context_id))
                .max_by_key(|(_, home)| home.member_count)
                .map(|(home_id, _)| *home_id)
        };

        let home_id = target_home_id.ok_or_else(|| {
            AuraError::from(WorkflowError::MissingAuthoritativeHomeProjection {
                context: context_id.to_string(),
            })
        })?;
        let home = homes.home_mut(&home_id).ok_or_else(|| {
            AuraError::permission_denied("Set channel mode requires a valid home context")
        })?;
        require_mode_authority(home)?;
        home.mode_flags = Some(flags);
        Ok(())
    })
    .await
}

async fn set_channel_mode_bound(
    app_core: &Arc<RwLock<AppCore>>,
    binding: AuthoritativeChannelBinding,
    flags: String,
) -> Result<(), AuraError> {
    try_update_homes_projection_observed(app_core, |homes| {
        let target_home_id = if homes.has_home(&binding.channel_id) {
            Some(binding.channel_id)
        } else {
            homes
                .iter()
                .filter(|(_, home)| home.context_id == Some(binding.context_id))
                .max_by_key(|(_, home)| home.member_count)
                .map(|(home_id, _)| *home_id)
        };

        let home_id = target_home_id.ok_or_else(|| {
            AuraError::from(WorkflowError::MissingAuthoritativeHomeProjection {
                context: binding.context_id.to_string(),
            })
        })?;
        let home = homes.home_mut(&home_id).ok_or_else(|| {
            AuraError::permission_denied("Set channel mode requires a valid home context")
        })?;
        require_mode_authority(home)?;
        home.context_id = Some(binding.context_id);
        home.mode_flags = Some(flags);
        Ok(())
    })
    .await
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

    let normalized_k = try_update_recovery_projection_observed(app_core, |recovery| {
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
        Ok(normalized_k)
    })
    .await?;

    let reactive = app_core.read().await.reactive().clone();
    let _ = reactive
        .update_signal(&*SETTINGS_SIGNAL, |state| {
            state.threshold_k = normalized_k;
            state.threshold_n = threshold_n;
            Ok::<(), AuraError>(())
        })
        .await
        .map_err(|error| AuraError::internal(error.to_string()))??;

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
    use crate::views::home::{HomeState, HomesState};
    use crate::workflows::signals::{emit_signal, read_signal};
    use crate::AppConfig;
    use aura_core::{crypto::hash::hash, AuthorityId, ChannelId, ContextId};

    #[tokio::test]
    async fn test_get_settings_default() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        // Workflows assume reactive signals are initialized.
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let settings = get_settings(&app_core).await.unwrap();
        assert_eq!(settings.threshold_k, 0);
        assert_eq!(settings.threshold_n, 0);
    }

    #[tokio::test]
    async fn test_update_mfa_policy_without_runtime() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

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
        runtime.set_materialized_channel_name_matches("general", vec![channel_id]);
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
            .set_materialized_channel_name_matches(&target_channel_name, vec![target_channel_id]);

        let error = set_channel_mode(&app_core, target_channel_name, "+m".to_string())
            .await
            .expect_err("mode update should fail without a channel-scoped home context");
        assert!(!error.to_string().is_empty());
    }
}

use crate::tui::hooks::{AppCoreContext, AppSnapshotAvailability};
use crate::tui::screens::app::subscriptions::SharedDevices;
use crate::tui::types::{AuthorityInfo, Device};
use aura_app::harness_mode_enabled;
use aura_app::ui::signals::SETTINGS_SIGNAL;
use aura_app::ui::workflows::settings::refresh_settings_from_runtime;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::{
    execute_with_retry_budget, ExponentialBackoffPolicy, RetryBudgetPolicy, TimeoutExecutionProfile,
};
use aura_effects::time::PhysicalTimeHandler;

pub(super) async fn effect_sleep(duration: std::time::Duration) {
    let _ = PhysicalTimeHandler::new()
        .sleep_ms(duration.as_millis() as u64)
        .await;
}

#[allow(clippy::expect_used)]
pub(super) fn shell_retry_policy() -> RetryBudgetPolicy {
    let profile = if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    };
    let base = RetryBudgetPolicy::new(
        200,
        ExponentialBackoffPolicy::new(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_secs(2),
            profile.jitter(),
        )
        .expect("shell backoff policy must be valid"),
    );
    profile
        .apply_retry_policy(&base)
        .expect("shell retry policy must scale")
}

pub(super) async fn authoritative_app_snapshot_with_retry(
    app_ctx: &AppCoreContext,
    context: &'static str,
) -> Result<Box<aura_app::ui::types::StateSnapshot>, String> {
    let retry_policy = shell_retry_policy();
    let time = PhysicalTimeHandler::new();
    execute_with_retry_budget(&time, &retry_policy, |_attempt| {
        let app_ctx = app_ctx.clone();
        async move {
            match app_ctx.snapshot() {
                AppSnapshotAvailability::Available(snapshot) => Ok(snapshot),
                AppSnapshotAvailability::Contended => Err("state lock contended".to_string()),
            }
        }
    })
    .await
    .map_err(|error| format!("{context}: {error}"))
}

pub(super) async fn authoritative_settings_devices_for_command(
    app_ctx: &AppCoreContext,
    shared_devices: &SharedDevices,
) -> Vec<Device> {
    let shared = shared_devices.read().clone();
    if harness_mode_enabled() {
        let _ = refresh_settings_from_runtime(app_ctx.app_core.raw()).await;
    }
    let mut from_signal = {
        let core = app_ctx.app_core.raw().read().await;
        core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
    };

    if from_signal
        .as_ref()
        .map_or(true, |settings_state| settings_state.devices.is_empty())
    {
        let _ = refresh_settings_from_runtime(app_ctx.app_core.raw()).await;
        from_signal = {
            let core = app_ctx.app_core.raw().read().await;
            core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
        };
    }

    if let Some(settings_state) = from_signal {
        let devices = settings_state
            .devices
            .iter()
            .map(|device| Device {
                id: device.id.to_string(),
                name: device.name.clone(),
                is_current: device.is_current,
                last_seen: device.last_seen,
            })
            .collect::<Vec<_>>();
        if !devices.is_empty() {
            *shared_devices.write() = devices.clone();
        }
        return devices;
    }

    shared
}

pub(super) async fn authoritative_settings_authorities_for_command(
    app_ctx: &AppCoreContext,
) -> (Vec<AuthorityInfo>, usize) {
    let from_signal = {
        let core = app_ctx.app_core.raw().read().await;
        core.reactive().read(&*SETTINGS_SIGNAL).await.ok()
    };

    if let Some(settings_state) = from_signal {
        let current_index = settings_state
            .authorities
            .iter()
            .position(|authority| authority.is_current)
            .unwrap_or(0);
        let authorities = settings_state
            .authorities
            .iter()
            .map(|authority| {
                let info = AuthorityInfo::new(
                    authority.id.to_string(),
                    authority.nickname_suggestion.clone(),
                );
                if authority.is_current {
                    info.current()
                } else {
                    info
                }
            })
            .collect::<Vec<_>>();
        return (authorities, current_index);
    }

    (Vec::new(), 0)
}

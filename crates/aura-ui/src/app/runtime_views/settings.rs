use crate::model::UiController;
use aura_app::signal_defs::SettingsState;
use aura_app::ui::signals::{RECOVERY_SIGNAL, SETTINGS_SIGNAL};
use aura_app::ui::types::RecoveryState;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::AuthorityId;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct SettingsRuntimeDevice {
    pub(in crate::app) id: String,
    pub(in crate::app) name: String,
    pub(in crate::app) is_current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct SettingsRuntimeAuthority {
    pub(in crate::app) id: AuthorityId,
    pub(in crate::app) label: String,
    pub(in crate::app) is_current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct SettingsRuntimeView {
    pub(in crate::app) loaded: bool,
    pub(in crate::app) nickname: String,
    pub(in crate::app) authority_id: String,
    pub(in crate::app) threshold_k: u8,
    pub(in crate::app) threshold_n: u8,
    pub(in crate::app) guardian_count: usize,
    pub(in crate::app) active_recovery_label: String,
    pub(in crate::app) pending_recovery_requests: usize,
    pub(in crate::app) guardian_binding_count: usize,
    pub(in crate::app) mfa_policy: String,
    pub(in crate::app) devices: Vec<SettingsRuntimeDevice>,
    pub(in crate::app) authorities: Vec<SettingsRuntimeAuthority>,
}

fn build_settings_runtime_view(
    settings: SettingsState,
    recovery: RecoveryState,
) -> SettingsRuntimeView {
    let devices = settings
        .devices
        .iter()
        .map(|device| SettingsRuntimeDevice {
            id: device.id.to_string(),
            name: if device.name.trim().is_empty() {
                let short = device.id.to_string().chars().take(8).collect::<String>();
                format!("Device {short}")
            } else {
                device.name.clone()
            },
            is_current: device.is_current,
        })
        .collect();
    let authorities = settings
        .authorities
        .iter()
        .map(|authority| SettingsRuntimeAuthority {
            label: if authority.nickname_suggestion.trim().is_empty() {
                authority.id.to_string()
            } else {
                authority.nickname_suggestion.clone()
            },
            id: authority.id,
            is_current: authority.is_current,
        })
        .collect();

    let active_recovery_label = recovery
        .active_recovery()
        .map(|process| format!("{:?}", process.status))
        .unwrap_or_else(|| "Idle".to_string());

    SettingsRuntimeView {
        loaded: true,
        nickname: settings.nickname_suggestion,
        authority_id: settings.authority_id,
        threshold_k: settings.threshold_k,
        threshold_n: settings.threshold_n,
        guardian_count: recovery.guardian_count(),
        active_recovery_label,
        pending_recovery_requests: recovery.pending_requests().len(),
        guardian_binding_count: recovery.guardian_binding_count(),
        mfa_policy: settings.mfa_policy,
        devices,
        authorities,
    }
}

pub(in crate::app) async fn load_settings_runtime_view(
    controller: Arc<UiController>,
) -> SettingsRuntimeView {
    let settings = {
        let core = controller.app_core().read().await;
        core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default()
    };
    let recovery = {
        let core = controller.app_core().read().await;
        core.read(&*RECOVERY_SIGNAL).await.unwrap_or_default()
    };
    let runtime = build_settings_runtime_view(settings, recovery);
    controller.sync_runtime_profile(runtime.authority_id.clone(), runtime.nickname.clone());
    controller.sync_runtime_devices(
        runtime
            .devices
            .iter()
            .map(|device| (device.name.clone(), device.is_current))
            .collect(),
    );
    controller.sync_runtime_authorities(
        runtime
            .authorities
            .iter()
            .map(|authority| (authority.id, authority.label.clone(), authority.is_current))
            .collect(),
    );
    runtime
}

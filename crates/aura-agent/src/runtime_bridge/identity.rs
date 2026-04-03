use super::{service_unavailable_with_detail, AgentRuntimeBridge};
use aura_app::runtime_bridge::{
    AuthenticationStatus, BridgeAuthorityInfo, BridgeDeviceInfo, SettingsBridgeState,
};
use aura_app::ui::workflows::authority::{authority_key_prefix, deserialize_authority};
use aura_app::views::naming::EffectiveName;
use aura_app::IntentError;
use aura_core::effects::{PhysicalTimeEffects, StorageCoreEffects, ThresholdSigningEffects};
use aura_core::tree::metadata::DeviceLeafMetadata;
use aura_core::tree::LeafRole;
use aura_protocol::effects::TreeEffects;
use std::collections::HashSet;

const RUNTIME_BRIDGE_IDENTITY_SETTINGS_QUERY_CAPABILITY: &str =
    "runtime_bridge_identity_settings_query";
const RUNTIME_BRIDGE_IDENTITY_DEVICE_QUERY_CAPABILITY: &str =
    "runtime_bridge_identity_device_query";
const RUNTIME_BRIDGE_IDENTITY_AUTHORITY_QUERY_CAPABILITY: &str =
    "runtime_bridge_identity_authority_query";
const RUNTIME_BRIDGE_IDENTITY_NICKNAME_MUTATION_CAPABILITY: &str =
    "runtime_bridge_identity_nickname_mutation";
const RUNTIME_BRIDGE_IDENTITY_MFA_POLICY_MUTATION_CAPABILITY: &str =
    "runtime_bridge_identity_mfa_policy_mutation";
const RUNTIME_BRIDGE_IDENTITY_TIME_QUERY_CAPABILITY: &str = "runtime_bridge_identity_time_query";
const RUNTIME_BRIDGE_IDENTITY_SLEEP_CAPABILITY: &str = "runtime_bridge_identity_sleep";
const RUNTIME_BRIDGE_IDENTITY_AUTHENTICATION_QUERY_CAPABILITY: &str =
    "runtime_bridge_identity_authentication_query";

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_settings_query",
    family = "runtime_helper"
)]
pub(super) async fn get_settings(
    bridge: &AgentRuntimeBridge,
) -> Result<SettingsBridgeState, IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_SETTINGS_QUERY_CAPABILITY;
    let device_count = list_devices(bridge).await?.len();
    let threshold_signing = bridge.agent.threshold_signing();
    let authority_id = bridge.agent.authority_id();
    let (threshold_k, threshold_n) =
        if let Some(config) = threshold_signing.threshold_config(&authority_id).await {
            (config.threshold, config.total_participants)
        } else {
            (0, 0)
        };
    let contact_count = bridge
        .agent
        .invitations()
        .map_err(|e| service_unavailable_with_detail("invitation_service", e))?
        .list_with_storage()
        .await
        .iter()
        .filter(|inv| {
            matches!(
                inv.invitation_type,
                crate::handlers::invitation::InvitationType::Contact { .. }
            ) && inv.status == crate::handlers::invitation::InvitationStatus::Accepted
        })
        .count();
    let (nickname_suggestion, mfa_policy) = match bridge.try_load_account_config().await {
        Ok(Some((_key, config))) => (
            config.nickname_suggestion.unwrap_or_default(),
            config.mfa_policy.unwrap_or_else(|| "disabled".to_string()),
        ),
        Ok(None) => (String::new(), "disabled".to_string()),
        Err(error) => return Err(error),
    };

    Ok(SettingsBridgeState {
        nickname_suggestion,
        mfa_policy,
        threshold_k,
        threshold_n,
        device_count,
        contact_count,
    })
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_device_query",
    family = "runtime_helper"
)]
pub(super) async fn list_devices(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<BridgeDeviceInfo>, IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_DEVICE_QUERY_CAPABILITY;
    let effects = bridge.agent.runtime().effects();
    let current_device = bridge.agent.context().device_id();
    let state = effects.get_current_state().await.map_err(|e| {
        IntentError::internal_error(format!("Failed to read current device list: {e}"))
    })?;

    let mut devices: Vec<BridgeDeviceInfo> = state
        .leaves
        .values()
        .filter(|leaf| leaf.role == LeafRole::Device)
        .map(|leaf| {
            let device = BridgeDeviceInfo {
                id: leaf.device_id,
                name: String::new(),
                nickname: None,
                nickname_suggestion: DeviceLeafMetadata::decode(&leaf.meta)
                    .ok()
                    .and_then(|meta| meta.nickname_suggestion),
                is_current: leaf.device_id == current_device,
                last_seen: None,
            };
            BridgeDeviceInfo {
                name: device.effective_name(),
                ..device
            }
        })
        .collect();

    if !devices.iter().any(|device| device.is_current) {
        let device = BridgeDeviceInfo {
            id: current_device,
            name: String::new(),
            nickname: None,
            nickname_suggestion: None,
            is_current: true,
            last_seen: None,
        };
        devices.insert(
            0,
            BridgeDeviceInfo {
                name: device.effective_name(),
                ..device
            },
        );
    }

    Ok(devices)
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_authority_query",
    family = "runtime_helper"
)]
pub(super) async fn list_authorities(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<BridgeAuthorityInfo>, IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_AUTHORITY_QUERY_CAPABILITY;
    let current_id = bridge.agent.authority_id();
    let current_nickname = match bridge.try_load_account_config().await {
        Ok(Some((_key, config))) => config
            .nickname_suggestion
            .filter(|value| !value.trim().is_empty()),
        Ok(None) => None,
        Err(error) => return Err(error),
    };

    let mut authorities = vec![BridgeAuthorityInfo {
        id: current_id,
        nickname_suggestion: current_nickname,
        is_current: true,
    }];
    let mut seen = HashSet::from([current_id]);
    let effects = bridge.agent.runtime().effects();
    let keys = effects
        .list_keys(Some(authority_key_prefix()))
        .await
        .map_err(|error| {
            IntentError::internal_error(format!("Failed to list stored authorities: {error}"))
        })?;

    for key in keys {
        let Some(bytes) = effects.retrieve(&key).await.map_err(|error| {
            IntentError::internal_error(format!("Failed to read authority record {key}: {error}"))
        })?
        else {
            continue;
        };

        let record = deserialize_authority(&bytes).map_err(|error| {
            IntentError::internal_error(format!("Failed to decode authority record {key}: {error}"))
        })?;

        if !seen.insert(record.authority_id) {
            continue;
        }

        authorities.push(BridgeAuthorityInfo {
            id: record.authority_id,
            nickname_suggestion: None,
            is_current: record.authority_id == current_id,
        });
    }

    authorities.sort_by(|left, right| {
        right
            .is_current
            .cmp(&left.is_current)
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    Ok(authorities)
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_nickname_mutation",
    family = "runtime_helper"
)]
pub(super) async fn set_nickname_suggestion(
    bridge: &AgentRuntimeBridge,
    name: &str,
) -> Result<(), IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_NICKNAME_MUTATION_CAPABILITY;
    let (key, mut config) = bridge.load_account_config().await?;
    config.nickname_suggestion = Some(name.to_string());
    bridge.store_account_config(&key, &config).await
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_mfa_policy_mutation",
    family = "runtime_helper"
)]
pub(super) async fn set_mfa_policy(
    bridge: &AgentRuntimeBridge,
    policy: &str,
) -> Result<(), IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_MFA_POLICY_MUTATION_CAPABILITY;
    let (key, mut config) = bridge.load_account_config().await?;
    config.mfa_policy = Some(policy.to_string());
    bridge.store_account_config(&key, &config).await
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_time_query",
    family = "runtime_helper"
)]
pub(super) async fn current_time_ms(bridge: &AgentRuntimeBridge) -> Result<u64, IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_TIME_QUERY_CAPABILITY;
    let effects = bridge.agent.runtime().effects();
    let time = effects
        .physical_time()
        .await
        .map_err(|e| service_unavailable_with_detail("physical_time", e))?;
    Ok(time.ts_ms)
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_sleep",
    family = "runtime_helper"
)]
pub(super) async fn sleep_ms(bridge: &AgentRuntimeBridge, ms: u64) {
    let _ = RUNTIME_BRIDGE_IDENTITY_SLEEP_CAPABILITY;
    let effects = bridge.agent.runtime().effects();
    let _ = effects.sleep_ms(ms).await;
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_identity_authentication_query",
    family = "runtime_helper"
)]
pub(super) async fn authentication_status(
    bridge: &AgentRuntimeBridge,
) -> Result<AuthenticationStatus, IntentError> {
    let _ = RUNTIME_BRIDGE_IDENTITY_AUTHENTICATION_QUERY_CAPABILITY;
    let auth_service = bridge
        .agent
        .auth()
        .map_err(|error| IntentError::internal_error(error.to_string()))?;
    let status = auth_service
        .authentication_status()
        .await
        .map_err(|error| IntentError::internal_error(error.to_string()))?;
    Ok(AuthenticationStatus::Authenticated {
        authority_id: status.authority_id,
        device_id: status.device_id,
    })
}

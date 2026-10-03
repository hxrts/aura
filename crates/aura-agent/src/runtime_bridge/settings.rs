use super::error_boundary::bridge_runtime_internal;
use super::AgentRuntimeBridge;
use aura_app::runtime_bridge::RuntimeBridgeError;
use aura_app::ui::workflows::authority::{
    authority_storage_key, deserialize_authority_required, AuthorityRecord,
};
use aura_core::effects::{PhysicalTimeEffects, StorageCoreEffects};
use aura_core::types::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

const ACCOUNT_CONFIG_KEYS: [&str; 2] = ["account.json", "demo-account.json"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct StoredAccountConfig {
    #[serde(default)]
    pub(super) authority_id: Option<String>,
    #[serde(default)]
    pub(super) context_id: Option<String>,
    #[serde(default)]
    pub(super) nickname_suggestion: Option<String>,
    #[serde(default)]
    pub(super) mfa_policy: Option<String>,
    #[serde(default)]
    pub(super) created_at: Option<u64>,
}

impl AgentRuntimeBridge {
    pub(super) async fn has_account_config(&self) -> Result<bool, RuntimeBridgeError> {
        Ok(
            read_account_config_required(self.agent.runtime().effects().as_ref())
                .await?
                .is_some(),
        )
    }

    pub(super) async fn initialize_account(
        &self,
        nickname_suggestion: &str,
    ) -> Result<(), RuntimeBridgeError> {
        let identity = self.agent.context();
        let authority_id = identity.authority_id();
        let context_id = identity.default_context_id();
        let effects = self.agent.runtime().effects();
        let _profile = effects.enrollment_profile_handoff_guard().await;
        let created_at = effects
            .physical_time()
            .await
            .map_err(|error| {
                bridge_runtime_internal("Determine account creation time failed", error)
            })?
            .ts_ms;
        // Only actual absence permits initial configuration creation.
        let (key, mut config) = read_account_config_required(effects.as_ref())
            .await?
            .unwrap_or_else(|| ("account.json".to_owned(), StoredAccountConfig::default()));
        config.authority_id = Some(authority_id.uuid().to_string());
        config.context_id = Some(context_id.uuid().to_string());
        config.nickname_suggestion = Some(nickname_suggestion.to_owned());
        config.created_at = Some(config.created_at.unwrap_or(created_at));
        self.ensure_authority_record(authority_id, created_at)
            .await?;
        write_account_config_required(effects.as_ref(), &key, &config).await
    }

    async fn ensure_authority_record(
        &self,
        authority_id: AuthorityId,
        created_at: u64,
    ) -> Result<(), RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        let key = authority_storage_key(&authority_id);
        let existing = effects.retrieve(&key).await.map_err(|error| {
            bridge_runtime_internal(
                "Read authority record failed",
                aura_core::AuraError::Storage {
                    message: format!("authority record {key}"),
                    source: Some(std::sync::Arc::new(error)),
                },
            )
        })?;
        if let Some(bytes) = existing {
            let record = deserialize_authority_required(&bytes).map_err(|error| {
                bridge_runtime_internal("Decode authority record failed", error)
            })?;
            if record.authority_id != authority_id {
                return Err(bridge_runtime_internal(
                    "Validate authority record failed",
                    aura_core::AuraError::invalid(
                        "stored authority record has a different authority",
                    ),
                ));
            }
            return Ok(());
        }
        let bytes = serde_json::to_vec(&AuthorityRecord::new(authority_id, 1, created_at))
            .map_err(|error| bridge_runtime_internal("Serialize authority record failed", error))?;
        effects.store(&key, bytes).await.map_err(|error| {
            bridge_runtime_internal(
                "Persist authority record failed",
                aura_core::AuraError::Storage {
                    message: format!("authority record {key}"),
                    source: Some(std::sync::Arc::new(error)),
                },
            )
        })
    }
}

/// Required identity/settings read on the existing runtime effect owner.
/// Account absence is represented by None; IO and codec failures retain causes.
pub(super) async fn read_account_config_required(
    effects: &crate::runtime::AuraEffectSystem,
) -> Result<Option<(String, StoredAccountConfig)>, aura_app::runtime_bridge::RuntimeBridgeError> {
    use super::error_boundary::bridge_runtime_internal;
    for key in ACCOUNT_CONFIG_KEYS {
        let bytes = effects.retrieve(key).await.map_err(|error| {
            bridge_runtime_internal(
                "Read account config failed",
                aura_core::AuraError::Storage {
                    message: format!("account config {key}"),
                    source: Some(std::sync::Arc::new(error)),
                },
            )
        })?;
        let Some(bytes) = bytes else {
            continue;
        };
        let config = decode_account_config_required(key, &bytes)?;
        return Ok(Some((key.to_owned(), config)));
    }
    Ok(None)
}

fn decode_account_config_required(
    key: &str,
    bytes: &[u8],
) -> Result<StoredAccountConfig, aura_app::runtime_bridge::RuntimeBridgeError> {
    use super::error_boundary::bridge_runtime_internal;
    if bytes.len() > 131_072 {
        return Err(bridge_runtime_internal(
            "Parse account config failed",
            aura_core::AuraError::invalid("account configuration exceeds bounds"),
        ));
    }
    serde_json::from_slice(bytes).map_err(|error| {
        bridge_runtime_internal(
            "Parse account config failed",
            aura_core::AuraError::Serialization {
                message: format!("account config {key}"),
                source: Some(std::sync::Arc::new(error)),
            },
        )
    })
}

pub(super) async fn write_account_config_required(
    effects: &crate::runtime::AuraEffectSystem,
    key: &str,
    config: &StoredAccountConfig,
) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
    use super::error_boundary::bridge_runtime_internal;
    let bytes = serde_json::to_vec(config)
        .map_err(|error| bridge_runtime_internal("Serialize account config failed", error))?;
    if bytes.len() > 131_072 {
        return Err(bridge_runtime_internal(
            "Write account config failed",
            aura_core::AuraError::invalid("account configuration exceeds bounds"),
        ));
    }
    effects.store(key, bytes).await.map_err(|error| {
        bridge_runtime_internal(
            "Write account config failed",
            aura_core::AuraError::Storage {
                message: format!("account config {key}"),
                source: Some(std::sync::Arc::new(error)),
            },
        )
    })
}

#[cfg(test)]
mod required_identity_settings_tests {
    use super::*;
    #[test]
    fn corrupt_required_account_config_retains_original_json_cause() {
        let error = decode_account_config_required("account.json", b"not-json")
            .expect_err("reject corrupt account config");
        assert_eq!(
            error.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Serialization
        );
        let mut source = std::error::Error::source(&error);
        let mut found = false;
        while let Some(cause) = source {
            found |= cause.is::<serde_json::Error>();
            source = cause.source();
        }
        assert!(found, "native JSON source must survive the bridge");
    }
    #[test]
    fn required_account_config_rejects_oversized_bytes_before_codec() {
        let bytes = vec![b' '; 131_073];
        let error = decode_account_config_required("account.json", &bytes)
            .expect_err("reject oversized config");
        assert_eq!(
            error.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Validation
        );
    }
}

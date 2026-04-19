use super::error_boundary::{bridge_internal, bridge_storage};
use super::AgentRuntimeBridge;
use aura_app::ui::workflows::authority::{
    authority_storage_key, serialize_authority, AuthorityRecord,
};
use aura_app::IntentError;
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
    pub(super) async fn has_account_config(&self) -> Result<bool, IntentError> {
        Ok(self.try_load_account_config().await?.is_some())
    }

    pub(super) async fn initialize_account(
        &self,
        nickname_suggestion: &str,
    ) -> Result<(), IntentError> {
        let authority_id = self.agent.authority_id();
        let context_id = crate::core::default_context_id_for_authority(authority_id);
        let effects = self.agent.runtime().effects();
        let created_at = effects
            .physical_time()
            .await
            .map_err(|error| bridge_internal("Determine account creation time failed", error))?
            .ts_ms;

        let (key, mut config) = self
            .try_load_account_config()
            .await?
            .unwrap_or_else(|| ("account.json".to_string(), StoredAccountConfig::default()));
        config.authority_id = Some(authority_id.to_string());
        config.context_id = Some(context_id.to_string());
        config.nickname_suggestion = Some(nickname_suggestion.to_string());
        config.created_at = Some(config.created_at.unwrap_or(created_at));
        self.store_account_config(&key, &config).await?;

        self.ensure_authority_record(authority_id, created_at)
            .await?;
        Ok(())
    }

    async fn ensure_authority_record(
        &self,
        authority_id: AuthorityId,
        created_at: u64,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let key = authority_storage_key(&authority_id);

        if effects
            .retrieve(&key)
            .await
            .map_err(|error| {
                bridge_storage("Read authority record failed", format!("{key}: {error}"))
            })?
            .is_some()
        {
            return Ok(());
        }

        let bytes = serialize_authority(&AuthorityRecord::new(authority_id, 1, created_at))
            .map_err(|error| bridge_internal("Serialize authority record failed", error))?;
        effects.store(&key, bytes).await.map_err(|error| {
            bridge_storage("Persist authority record failed", format!("{key}: {error}"))
        })?;
        Ok(())
    }

    pub(super) async fn try_load_account_config(
        &self,
    ) -> Result<Option<(String, StoredAccountConfig)>, IntentError> {
        let effects = self.agent.runtime().effects();

        for key in ACCOUNT_CONFIG_KEYS {
            let bytes = effects.retrieve(key).await.map_err(|error| {
                bridge_storage("Read account config failed", format!("{key}: {error}"))
            })?;

            let Some(bytes) = bytes else {
                continue;
            };

            let config: StoredAccountConfig = serde_json::from_slice(&bytes).map_err(|error| {
                bridge_internal("Parse account config failed", format!("{key}: {error}"))
            })?;

            return Ok(Some((key.to_string(), config)));
        }

        Ok(None)
    }

    pub(super) async fn load_account_config(
        &self,
    ) -> Result<(String, StoredAccountConfig), IntentError> {
        let Some((key, config)) = self.try_load_account_config().await? else {
            return Err(IntentError::storage_error(
                "No account configuration found".to_string(),
            ));
        };

        Ok((key, config))
    }

    pub(super) async fn store_account_config(
        &self,
        key: &str,
        config: &StoredAccountConfig,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();

        let bytes = serde_json::to_vec(config).map_err(|error| {
            bridge_internal("Serialize account config failed", format!("{key}: {error}"))
        })?;

        effects.store(key, bytes).await.map_err(|error| {
            bridge_storage("Write account config failed", format!("{key}: {error}"))
        })?;

        Ok(())
    }
}

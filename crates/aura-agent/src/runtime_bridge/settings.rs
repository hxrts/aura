use super::AgentRuntimeBridge;
use aura_app::IntentError;
use aura_core::effects::StorageCoreEffects;
use serde::{Deserialize, Serialize};

const ACCOUNT_CONFIG_KEYS: [&str; 2] = ["account.json", "demo-account.json"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct StoredAccountConfig {
    #[serde(default)]
    pub(super) authority_id: Option<String>,
    #[serde(default)]
    pub(super) context_id: Option<String>,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) mfa_policy: Option<String>,
    #[serde(default)]
    pub(super) created_at: Option<u64>,
}

impl AgentRuntimeBridge {
    pub(super) async fn try_load_account_config(
        &self,
    ) -> Result<Option<(String, StoredAccountConfig)>, IntentError> {
        let effects = self.agent.runtime().effects();

        for key in ACCOUNT_CONFIG_KEYS {
            let bytes = effects
                .retrieve(key)
                .await
                .map_err(|e| IntentError::storage_error(format!("Failed to read {key}: {e}")))?;

            let Some(bytes) = bytes else {
                continue;
            };

            let config: StoredAccountConfig = serde_json::from_slice(&bytes)
                .map_err(|e| IntentError::internal_error(format!("Failed to parse {key}: {e}")))?;

            return Ok(Some((key.to_string(), config)));
        }

        Ok(None)
    }

    pub(super) async fn load_account_config(&self) -> Result<(String, StoredAccountConfig), IntentError> {
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

        let bytes = serde_json::to_vec(config)
            .map_err(|e| IntentError::internal_error(format!("Failed to serialize {key}: {e}")))?;

        effects
            .store(key, bytes)
            .await
            .map_err(|e| IntentError::storage_error(format!("Failed to write {key}: {e}")))?;

        Ok(())
    }
}

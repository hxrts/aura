use async_lock::RwLock;
use aura_app::ui::types::AccountConfig;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::AppCore;
use aura_ui::FrontendUiOperation as WebUiOperation;
use std::sync::Arc;

use crate::error::WebUiError;
use crate::shell::storage::{WebLocalStorage, WebStorageScope};

fn normalized_nickname_suggestion(value: Option<String>) -> Option<String> {
    value.and_then(|nickname| {
        let trimmed = nickname.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn load_persisted_account_config(
    operation: WebUiOperation,
) -> Result<Option<AccountConfig>, WebUiError> {
    let Some(storage) = WebLocalStorage::optional_lookup(operation)? else {
        return Ok(None);
    };
    let storage_key = WebStorageScope::active().account_config_key();
    storage.load_json(
        &storage_key,
        operation,
        "WEB_ACCOUNT_CONFIG_READ_FAILED",
        "WEB_ACCOUNT_CONFIG_PARSE_FAILED",
        "persisted account config",
        "persisted account config",
    )
}

fn persist_account_config(
    operation: WebUiOperation,
    account_config: &AccountConfig,
) -> Result<(), WebUiError> {
    let storage_key = WebStorageScope::active().account_config_key();
    WebLocalStorage::required(operation)?.set_json(
        &storage_key,
        account_config,
        operation,
        "WEB_ACCOUNT_CONFIG_SERIALIZE_FAILED",
        "WEB_ACCOUNT_CONFIG_PERSIST_FAILED",
        "persisted account config",
        "persisted account config",
    )
}

pub(crate) async fn persist_runtime_account_config(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_override: Option<String>,
    operation: WebUiOperation,
) -> Result<AccountConfig, WebUiError> {
    let existing = load_persisted_account_config(operation)?;
    let runtime = runtime_workflows::require_runtime(app_core)
        .await
        .map_err(|error| {
            WebUiError::operation(
                operation,
                "WEB_ACCOUNT_CONFIG_RUNTIME_REQUIRED_FAILED",
                error.to_string(),
            )
        })?;
    let authority_id = runtime.authority_id();
    let matching_existing = existing
        .as_ref()
        .filter(|account_config| account_config.authority_id == authority_id);
    let context_id = context_workflows::current_home_context(app_core)
        .await
        .map_err(|error| {
            WebUiError::operation(
                operation,
                "WEB_ACCOUNT_CONFIG_HOME_CONTEXT_REQUIRED_FAILED",
                error.to_string(),
            )
        })?;
    let settings_nickname = settings_workflows::get_settings(app_core)
        .await
        .ok()
        .and_then(|settings| normalized_nickname_suggestion(Some(settings.nickname_suggestion)));
    let nickname_suggestion = normalized_nickname_suggestion(nickname_override)
        .or(settings_nickname)
        .or_else(|| {
            matching_existing.and_then(|account_config| account_config.nickname_suggestion.clone())
        });
    let created_at = if let Some(account_config) = matching_existing {
        account_config.created_at
    } else {
        time_workflows::current_time_ms(app_core)
            .await
            .map_err(|error| {
                WebUiError::operation(
                    operation,
                    "WEB_ACCOUNT_CONFIG_CREATED_AT_FAILED",
                    error.to_string(),
                )
            })?
    };
    let account_config =
        AccountConfig::new(authority_id, context_id, nickname_suggestion, created_at);
    persist_account_config(operation, &account_config)?;
    Ok(account_config)
}

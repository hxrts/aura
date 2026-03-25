use async_lock::RwLock;
use aura_app::ui::types::{AccountConfig, WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX};
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::AppCore;
use aura_ui::FrontendUiOperation as WebUiOperation;
use std::sync::Arc;

use crate::active_storage_prefix;
use crate::error::WebUiError;

fn account_config_key(storage_prefix: &str) -> String {
    format!("{storage_prefix}{WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX}")
}

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
    let Some(window) = web_sys::window() else {
        return Ok(None);
    };
    let Some(storage) = window.local_storage().map_err(|error| {
        WebUiError::config(
            operation,
            "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
            format!("failed to access localStorage: {:?}", error),
        )
    })?
    else {
        return Ok(None);
    };

    let storage_key = account_config_key(&active_storage_prefix());
    let Some(raw) = storage.get_item(&storage_key).map_err(|error| {
        WebUiError::config(
            operation,
            "WEB_ACCOUNT_CONFIG_READ_FAILED",
            format!("failed to read persisted account config: {:?}", error),
        )
    })?
    else {
        return Ok(None);
    };

    serde_json::from_str(&raw).map(Some).map_err(|error| {
        WebUiError::config(
            operation,
            "WEB_ACCOUNT_CONFIG_PARSE_FAILED",
            format!("failed to parse persisted account config: {error}"),
        )
    })
}

fn persist_account_config(
    operation: WebUiOperation,
    account_config: &AccountConfig,
) -> Result<(), WebUiError> {
    let window = web_sys::window().ok_or_else(|| {
        WebUiError::config(
            operation,
            "WEB_WINDOW_UNAVAILABLE",
            "window is not available",
        )
    })?;
    let storage = window
        .local_storage()
        .map_err(|error| {
            WebUiError::config(
                operation,
                "WEB_LOCAL_STORAGE_UNAVAILABLE",
                format!("localStorage unavailable: {:?}", error),
            )
        })?
        .ok_or_else(|| {
            WebUiError::config(
                operation,
                "WEB_LOCAL_STORAGE_MISSING",
                "localStorage unavailable",
            )
        })?;
    let raw = serde_json::to_string(account_config).map_err(|error| {
        WebUiError::operation(
            operation,
            "WEB_ACCOUNT_CONFIG_SERIALIZE_FAILED",
            format!("failed to serialize persisted account config: {error}"),
        )
    })?;
    let storage_key = account_config_key(&active_storage_prefix());
    storage.set_item(&storage_key, &raw).map_err(|error| {
        WebUiError::config(
            operation,
            "WEB_ACCOUNT_CONFIG_PERSIST_FAILED",
            format!("failed to persist account config: {:?}", error),
        )
    })
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

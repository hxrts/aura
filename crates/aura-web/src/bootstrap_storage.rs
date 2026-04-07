use async_lock::RwLock;
use aura_agent::core::default_context_id_for_authority;
use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
use aura_app::ui::types::AccountConfig;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::AppCore;
use aura_core::types::identifiers::{AuthorityId, ContextId};
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

fn resolve_account_config_context_id(
    authority_id: AuthorityId,
    matching_existing: Option<&AccountConfig>,
    active_home_context: Option<ContextId>,
) -> ContextId {
    active_home_context
        .or_else(|| matching_existing.map(|account_config| account_config.context_id))
        .unwrap_or_else(|| default_context_id_for_authority(authority_id))
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
    let active_home_context = context_workflows::current_home_context(app_core).await.ok();
    let context_id =
        resolve_account_config_context_id(authority_id, matching_existing, active_home_context);
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

#[cfg(test)]
mod tests {
    use super::resolve_account_config_context_id;
    use aura_agent::core::default_context_id_for_authority;
    use aura_app::ui::types::AccountConfig;
    use aura_core::types::identifiers::{AuthorityId, ContextId};
    use std::str::FromStr;

    fn test_authority() -> AuthorityId {
        AuthorityId::from_str("authority-01234567-89ab-cdef-0123-456789abcdef")
            .unwrap_or_else(|error| panic!("valid authority id: {error}"))
    }

    fn test_context(seed: &str) -> ContextId {
        ContextId::from_str(seed).unwrap_or_else(|error| panic!("valid context id: {error}"))
    }

    #[test]
    fn account_config_context_prefers_active_home() {
        let authority_id = test_authority();
        let existing = AccountConfig::new(
            authority_id,
            test_context("11111111-2222-3333-4444-555555555555"),
            Some("Alice".to_string()),
            1,
        );
        let active_home = test_context("66666666-7777-8888-9999-aaaaaaaaaaaa");

        let resolved =
            resolve_account_config_context_id(authority_id, Some(&existing), Some(active_home));

        assert_eq!(resolved, active_home);
    }

    #[test]
    fn account_config_context_reuses_existing_when_no_home_exists() {
        let authority_id = test_authority();
        let existing_context = test_context("11111111-2222-3333-4444-555555555555");
        let existing =
            AccountConfig::new(authority_id, existing_context, Some("Alice".to_string()), 1);

        let resolved = resolve_account_config_context_id(authority_id, Some(&existing), None);

        assert_eq!(resolved, existing_context);
    }

    #[test]
    fn account_config_context_falls_back_to_authority_default_without_home_or_existing() {
        let authority_id = test_authority();

        let resolved = resolve_account_config_context_id(authority_id, None, None);

        assert_eq!(resolved, default_context_id_for_authority(authority_id));
    }
}

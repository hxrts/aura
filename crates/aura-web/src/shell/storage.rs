use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
use aura_app::ui::types::{
    BootstrapRuntimeIdentity, PendingAccountBootstrap, WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX,
    WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX, WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{log_web_error, WebUiError};

const WEB_STORAGE_PREFIX: &str = "aura_";
const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";
const HARNESS_TOKEN_QUERY_KEY: &str = "__aura_harness_token";
const DEMO_SURFACE_QUERY_KEY: &str = "__aura_demo_surface";
const BOOTSTRAP_BROKER_QUERY_KEY: &str = "__aura_bootstrap_broker";
const BOOTSTRAP_BROKER_AUTH_SESSION_KEY: &str = "aura_bootstrap_broker_auth";
const BOOTSTRAP_BROKER_INVITATION_SESSION_KEY: &str = "aura_bootstrap_broker_invitation";
const PENDING_ACCOUNT_BOOTSTRAP_TTL_MS: u64 = 30 * 60 * 1000;
const PENDING_DEVICE_ENROLLMENT_CODE_TTL_MS: u64 = 10 * 60 * 1000;
const MIN_HARNESS_TOKEN_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserStoredValueClass {
    RestartMetadata,
    ShortLivedMetadata,
    SensitiveOneShotSecret,
}

pub(crate) fn browser_stored_value_class(key: &str) -> BrowserStoredValueClass {
    if key.ends_with("pending_device_enrollment_code") {
        BrowserStoredValueClass::SensitiveOneShotSecret
    } else if key.ends_with(WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX) {
        BrowserStoredValueClass::ShortLivedMetadata
    } else {
        BrowserStoredValueClass::RestartMetadata
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExpiringBrowserRecord<T> {
    value: T,
    created_at_ms: u64,
    ttl_ms: u64,
}

impl<T> ExpiringBrowserRecord<T> {
    fn new(value: T, ttl_ms: u64) -> Self {
        Self {
            value,
            created_at_ms: browser_now_ms(),
            ttl_ms,
        }
    }

    fn is_expired(&self, now_ms: u64) -> bool {
        record_is_expired(self.created_at_ms, now_ms, self.ttl_ms)
    }
}

fn record_is_expired(created_at_ms: u64, now_ms: u64, ttl_ms: u64) -> bool {
    now_ms.saturating_sub(created_at_ms) > ttl_ms
}

fn browser_now_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}

fn pending_bootstrap_for_persistence(
    pending_bootstrap: &PendingAccountBootstrap,
) -> PendingAccountBootstrap {
    let mut persisted = pending_bootstrap.clone();
    persisted.device_enrollment_code = None;
    persisted
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WebStorageScope {
    prefix: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HarnessSession {
    pub(crate) instance_id: String,
    pub(crate) token: String,
}

impl WebStorageScope {
    pub(crate) fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    pub(crate) fn active() -> Self {
        if let Some(instance_id) = harness_instance_id() {
            let sanitized = sanitize_storage_segment(&instance_id);
            if !sanitized.is_empty() {
                return Self::new(format!("{WEB_STORAGE_PREFIX}{sanitized}_"));
            }
        }
        Self::new(WEB_STORAGE_PREFIX)
    }

    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    pub(crate) fn selected_runtime_identity_key(&self) -> String {
        format!(
            "{}{}",
            self.prefix, WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX
        )
    }

    pub(crate) fn pending_device_enrollment_code_key(&self) -> String {
        format!("{}pending_device_enrollment_code", self.prefix)
    }

    pub(crate) fn pending_account_bootstrap_key(&self) -> String {
        format!(
            "{}{}",
            self.prefix, WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX
        )
    }

    pub(crate) fn account_config_key(&self) -> String {
        format!("{}{}", self.prefix, WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX)
    }

    pub(crate) fn demo_tablet_enrollment_code_key(&self) -> String {
        format!("{}demo_tablet_enrollment_code", self.prefix)
    }
}

pub(crate) struct WebLocalStorage {
    storage: web_sys::Storage,
}

pub(crate) struct WebSessionStorage {
    storage: web_sys::Storage,
}

impl WebLocalStorage {
    pub(crate) fn optional_lookup(operation: WebUiOperation) -> Result<Option<Self>, WebUiError> {
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
        Ok(Some(Self { storage }))
    }

    pub(crate) fn required(operation: WebUiOperation) -> Result<Self, WebUiError> {
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
        Ok(Self { storage })
    }

    pub(crate) fn load_string(
        &self,
        key: &str,
        operation: WebUiOperation,
        read_error_code: &'static str,
        read_context: &'static str,
    ) -> Result<Option<String>, WebUiError> {
        self.storage.get_item(key).map_err(|error| {
            WebUiError::config(
                operation,
                read_error_code,
                format!("failed to read {read_context}: {:?}", error),
            )
        })
    }

    pub(crate) fn set_string(
        &self,
        key: &str,
        value: &str,
        operation: WebUiOperation,
        write_error_code: &'static str,
        write_context: &str,
    ) -> Result<(), WebUiError> {
        self.storage.set_item(key, value).map_err(|error| {
            WebUiError::config(
                operation,
                write_error_code,
                format!("failed to persist {write_context}: {:?}", error),
            )
        })
    }

    pub(crate) fn remove(
        &self,
        key: &str,
        operation: WebUiOperation,
        clear_error_code: &'static str,
        clear_context: &str,
    ) -> Result<(), WebUiError> {
        self.storage.remove_item(key).map_err(|error| {
            WebUiError::config(
                operation,
                clear_error_code,
                format!("failed to clear {clear_context}: {:?}", error),
            )
        })
    }

    pub(crate) fn load_json<T: DeserializeOwned>(
        &self,
        key: &str,
        operation: WebUiOperation,
        read_error_code: &'static str,
        parse_error_code: &'static str,
        read_context: &'static str,
        parse_context: &'static str,
    ) -> Result<Option<T>, WebUiError> {
        let Some(raw) = self.load_string(key, operation, read_error_code, read_context)? else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map(Some).map_err(|error| {
            WebUiError::config(
                operation,
                parse_error_code,
                format!("failed to parse {parse_context}: {error}"),
            )
        })
    }

    pub(crate) fn set_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        operation: WebUiOperation,
        serialize_error_code: &'static str,
        persist_error_code: &'static str,
        serialize_context: &'static str,
        persist_context: &str,
    ) -> Result<(), WebUiError> {
        let raw = serde_json::to_string(value).map_err(|error| {
            WebUiError::operation(
                operation,
                serialize_error_code,
                format!("failed to serialize {serialize_context}: {error}"),
            )
        })?;
        self.set_string(key, &raw, operation, persist_error_code, persist_context)
    }
}

impl WebSessionStorage {
    pub(crate) fn optional_lookup(operation: WebUiOperation) -> Result<Option<Self>, WebUiError> {
        let Some(window) = web_sys::window() else {
            return Ok(None);
        };
        let Some(storage) = window.session_storage().map_err(|error| {
            WebUiError::config(
                operation,
                "WEB_SESSION_STORAGE_LOOKUP_FAILED",
                format!("failed to access sessionStorage: {:?}", error),
            )
        })?
        else {
            return Ok(None);
        };
        Ok(Some(Self { storage }))
    }

    pub(crate) fn required(operation: WebUiOperation) -> Result<Self, WebUiError> {
        let window = web_sys::window().ok_or_else(|| {
            WebUiError::config(
                operation,
                "WEB_WINDOW_UNAVAILABLE",
                "window is not available",
            )
        })?;
        let storage = window
            .session_storage()
            .map_err(|error| {
                WebUiError::config(
                    operation,
                    "WEB_SESSION_STORAGE_UNAVAILABLE",
                    format!("sessionStorage unavailable: {:?}", error),
                )
            })?
            .ok_or_else(|| {
                WebUiError::config(
                    operation,
                    "WEB_SESSION_STORAGE_MISSING",
                    "sessionStorage unavailable",
                )
            })?;
        Ok(Self { storage })
    }

    fn load_string(
        &self,
        key: &str,
        operation: WebUiOperation,
        read_error_code: &'static str,
        read_context: &'static str,
    ) -> Result<Option<String>, WebUiError> {
        self.storage.get_item(key).map_err(|error| {
            WebUiError::config(
                operation,
                read_error_code,
                format!("failed to read {read_context}: {:?}", error),
            )
        })
    }

    fn set_string(
        &self,
        key: &str,
        value: &str,
        operation: WebUiOperation,
        write_error_code: &'static str,
        write_context: &str,
    ) -> Result<(), WebUiError> {
        self.storage.set_item(key, value).map_err(|error| {
            WebUiError::config(
                operation,
                write_error_code,
                format!("failed to persist {write_context}: {:?}", error),
            )
        })
    }

    fn remove(
        &self,
        key: &str,
        operation: WebUiOperation,
        clear_error_code: &'static str,
        clear_context: &str,
    ) -> Result<(), WebUiError> {
        self.storage.remove_item(key).map_err(|error| {
            WebUiError::config(
                operation,
                clear_error_code,
                format!("failed to clear {clear_context}: {:?}", error),
            )
        })
    }

    fn load_json<T: DeserializeOwned>(
        &self,
        key: &str,
        operation: WebUiOperation,
        read_error_code: &'static str,
        parse_error_code: &'static str,
        read_context: &'static str,
        parse_context: &'static str,
    ) -> Result<Option<T>, WebUiError> {
        let Some(raw) = self.load_string(key, operation, read_error_code, read_context)? else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map(Some).map_err(|error| {
            WebUiError::config(
                operation,
                parse_error_code,
                format!("failed to parse {parse_context}: {error}"),
            )
        })
    }

    fn set_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        operation: WebUiOperation,
        serialize_error_code: &'static str,
        persist_error_code: &'static str,
        serialize_context: &'static str,
        persist_context: &str,
    ) -> Result<(), WebUiError> {
        let raw = serde_json::to_string(value).map_err(|error| {
            WebUiError::operation(
                operation,
                serialize_error_code,
                format!("failed to serialize {serialize_context}: {error}"),
            )
        })?;
        self.set_string(key, &raw, operation, persist_error_code, persist_context)
    }
}

fn sanitize_storage_segment(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn harness_instance_id() -> Option<String> {
    harness_session().map(|session| session.instance_id)
}

pub(crate) fn harness_mode_enabled() -> bool {
    harness_session().is_some()
}

pub(crate) fn demo_surface() -> Option<String> {
    query_value(DEMO_SURFACE_QUERY_KEY)
}

pub(crate) fn bootstrap_broker_url() -> Option<String> {
    query_value(BOOTSTRAP_BROKER_QUERY_KEY)
}

pub(crate) fn bootstrap_broker_auth_token() -> Option<String> {
    session_value(BOOTSTRAP_BROKER_AUTH_SESSION_KEY)
}

pub(crate) fn bootstrap_broker_invitation_token() -> Option<String> {
    session_value(BOOTSTRAP_BROKER_INVITATION_SESSION_KEY)
}

fn session_value(key: &str) -> Option<String> {
    WebSessionStorage::optional_lookup(WebUiOperation::BootstrapController)
        .ok()
        .flatten()?
        .load_string(
            key,
            WebUiOperation::BootstrapController,
            "WEB_BOOTSTRAP_SESSION_READ_FAILED",
            "bootstrap broker session credential",
        )
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

fn query_value(target_key: &str) -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == target_key && !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn harness_token() -> Option<String> {
    if !cfg!(feature = "harness") {
        return None;
    }
    let token = query_value(HARNESS_TOKEN_QUERY_KEY)?;
    if token.len() < MIN_HARNESS_TOKEN_LEN {
        return None;
    }
    Some(token)
}

pub(crate) fn harness_session() -> Option<HarnessSession> {
    if !cfg!(feature = "harness") {
        return None;
    }
    let instance_id = query_value(HARNESS_INSTANCE_QUERY_KEY)?;
    if instance_id.is_empty() {
        return None;
    }
    let token = harness_token()?;
    Some(HarnessSession { instance_id, token })
}

pub(crate) fn dual_demo_web_enabled() -> bool {
    demo_surface().as_deref() == Some("web")
}

pub(crate) fn active_storage_prefix() -> String {
    WebStorageScope::active().prefix().to_string()
}

pub(crate) fn logged_optional<T>(result: Result<Option<T>, WebUiError>) -> Option<T> {
    match result {
        Ok(value) => value,
        Err(error) => {
            log_web_error("warn", &error);
            None
        }
    }
}

pub(crate) fn selected_runtime_identity_key(storage_prefix: &str) -> String {
    WebStorageScope::new(storage_prefix).selected_runtime_identity_key()
}

pub(crate) fn pending_device_enrollment_code_key(storage_prefix: &str) -> String {
    WebStorageScope::new(storage_prefix).pending_device_enrollment_code_key()
}

pub(crate) fn pending_account_bootstrap_key(storage_prefix: &str) -> String {
    WebStorageScope::new(storage_prefix).pending_account_bootstrap_key()
}

pub(crate) fn demo_tablet_enrollment_code_key(storage_prefix: &str) -> String {
    WebStorageScope::new(storage_prefix).demo_tablet_enrollment_code_key()
}

pub(crate) fn load_selected_runtime_identity(
    storage_key: &str,
) -> Result<Option<BootstrapRuntimeIdentity>, WebUiError> {
    let Some(storage) =
        WebLocalStorage::optional_lookup(WebUiOperation::LoadSelectedRuntimeIdentity)?
    else {
        return Ok(None);
    };
    storage.load_json(
        storage_key,
        WebUiOperation::LoadSelectedRuntimeIdentity,
        "WEB_RUNTIME_IDENTITY_READ_FAILED",
        "WEB_RUNTIME_IDENTITY_PARSE_FAILED",
        "selected runtime identity",
        "selected runtime identity",
    )
}

pub(crate) fn persist_selected_runtime_identity(
    storage_key: &str,
    identity: &BootstrapRuntimeIdentity,
) -> Result<(), WebUiError> {
    WebLocalStorage::required(WebUiOperation::PersistSelectedRuntimeIdentity)?.set_json(
        storage_key,
        identity,
        WebUiOperation::PersistSelectedRuntimeIdentity,
        "WEB_RUNTIME_IDENTITY_SERIALIZE_FAILED",
        "WEB_RUNTIME_IDENTITY_PERSIST_FAILED",
        "runtime identity",
        "selected runtime identity",
    )
}

pub(crate) fn clear_storage_key(storage_key: &str) -> Result<(), WebUiError> {
    WebLocalStorage::required(WebUiOperation::ClearStorageKey)?.remove(
        storage_key,
        WebUiOperation::ClearStorageKey,
        "WEB_STORAGE_CLEAR_FAILED",
        &format!("localStorage key {storage_key}"),
    )
}

pub(crate) fn load_pending_account_bootstrap(
    storage_key: &str,
) -> Result<Option<PendingAccountBootstrap>, WebUiError> {
    debug_assert_eq!(
        browser_stored_value_class(storage_key),
        BrowserStoredValueClass::ShortLivedMetadata
    );
    let Some(storage) =
        WebLocalStorage::optional_lookup(WebUiOperation::LoadPendingAccountBootstrap)?
    else {
        return Ok(None);
    };
    let Some(record) = storage.load_json::<ExpiringBrowserRecord<PendingAccountBootstrap>>(
        storage_key,
        WebUiOperation::LoadPendingAccountBootstrap,
        "WEB_PENDING_BOOTSTRAP_READ_FAILED",
        "WEB_PENDING_BOOTSTRAP_PARSE_FAILED",
        "pending account bootstrap",
        "pending account bootstrap",
    )?
    else {
        return Ok(None);
    };
    if record.is_expired(browser_now_ms()) {
        storage.remove(
            storage_key,
            WebUiOperation::LoadPendingAccountBootstrap,
            "WEB_PENDING_BOOTSTRAP_CLEAR_EXPIRED_FAILED",
            "expired pending account bootstrap",
        )?;
        return Ok(None);
    }
    Ok(Some(record.value))
}

pub(crate) fn persist_pending_account_bootstrap(
    storage_key: &str,
    pending_bootstrap: &PendingAccountBootstrap,
) -> Result<(), WebUiError> {
    debug_assert_eq!(
        browser_stored_value_class(storage_key),
        BrowserStoredValueClass::ShortLivedMetadata
    );
    let record = ExpiringBrowserRecord::new(
        pending_bootstrap_for_persistence(pending_bootstrap),
        PENDING_ACCOUNT_BOOTSTRAP_TTL_MS,
    );
    WebLocalStorage::required(WebUiOperation::PersistPendingAccountBootstrap)?.set_json(
        storage_key,
        &record,
        WebUiOperation::PersistPendingAccountBootstrap,
        "WEB_PENDING_BOOTSTRAP_SERIALIZE_FAILED",
        "WEB_PENDING_BOOTSTRAP_PERSIST_FAILED",
        "pending account bootstrap",
        "pending account bootstrap",
    )
}

pub(crate) fn load_pending_device_enrollment_code(
    storage_key: &str,
) -> Result<Option<String>, WebUiError> {
    debug_assert_eq!(
        browser_stored_value_class(storage_key),
        BrowserStoredValueClass::SensitiveOneShotSecret
    );
    let Some(storage) =
        WebSessionStorage::optional_lookup(WebUiOperation::LoadPendingDeviceEnrollmentCode)?
    else {
        return Ok(None);
    };
    let Some(record) = storage.load_json::<ExpiringBrowserRecord<String>>(
        storage_key,
        WebUiOperation::LoadPendingDeviceEnrollmentCode,
        "WEB_PENDING_DEVICE_ENROLLMENT_CODE_READ_FAILED",
        "WEB_PENDING_DEVICE_ENROLLMENT_CODE_PARSE_FAILED",
        "pending device enrollment code",
        "pending device enrollment code",
    )?
    else {
        return Ok(None);
    };
    if record.is_expired(browser_now_ms()) {
        storage.remove(
            storage_key,
            WebUiOperation::LoadPendingDeviceEnrollmentCode,
            "WEB_PENDING_ENROLLMENT_CLEAR_EXPIRED_FAILED",
            "expired pending device enrollment code",
        )?;
        return Ok(None);
    }
    Ok(Some(record.value))
}

pub(crate) fn persist_pending_device_enrollment_code(
    storage_key: &str,
    code: &str,
) -> Result<(), WebUiError> {
    debug_assert_eq!(
        browser_stored_value_class(storage_key),
        BrowserStoredValueClass::SensitiveOneShotSecret
    );
    let record =
        ExpiringBrowserRecord::new(code.to_string(), PENDING_DEVICE_ENROLLMENT_CODE_TTL_MS);
    WebSessionStorage::required(WebUiOperation::PersistPendingDeviceEnrollmentCode)?.set_json(
        storage_key,
        &record,
        WebUiOperation::PersistPendingDeviceEnrollmentCode,
        "WEB_PENDING_ENROLLMENT_SERIALIZE_FAILED",
        "WEB_PENDING_ENROLLMENT_PERSIST_FAILED",
        "pending device enrollment code",
        "pending device enrollment code",
    )
}

pub(crate) fn clear_pending_device_enrollment_code(storage_key: &str) -> Result<(), WebUiError> {
    debug_assert_eq!(
        browser_stored_value_class(storage_key),
        BrowserStoredValueClass::SensitiveOneShotSecret
    );
    WebSessionStorage::required(WebUiOperation::ClearPendingDeviceEnrollmentCode)?.remove(
        storage_key,
        WebUiOperation::ClearPendingDeviceEnrollmentCode,
        "WEB_PENDING_ENROLLMENT_CLEAR_FAILED",
        "pending device enrollment code",
    )
}

pub(crate) fn persist_demo_tablet_enrollment_code(
    storage_key: &str,
    code: &str,
) -> Result<(), WebUiError> {
    let record =
        ExpiringBrowserRecord::new(code.to_string(), PENDING_DEVICE_ENROLLMENT_CODE_TTL_MS);
    WebSessionStorage::required(WebUiOperation::PersistPendingDeviceEnrollmentCode)?.set_json(
        storage_key,
        &record,
        WebUiOperation::PersistPendingDeviceEnrollmentCode,
        "WEB_DEMO_TABLET_CODE_SERIALIZE_FAILED",
        "WEB_DEMO_TABLET_CODE_PERSIST_FAILED",
        "demo tablet enrollment code",
        "demo tablet enrollment code",
    )
}

pub(crate) fn clear_demo_tablet_enrollment_code(storage_key: &str) -> Result<(), WebUiError> {
    WebSessionStorage::required(WebUiOperation::ClearPendingDeviceEnrollmentCode)?.remove(
        storage_key,
        WebUiOperation::ClearPendingDeviceEnrollmentCode,
        "WEB_DEMO_TABLET_CODE_CLEAR_FAILED",
        "demo tablet enrollment code",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_storage_classifies_sensitive_and_restart_values() {
        assert_eq!(
            browser_stored_value_class("aura_pending_device_enrollment_code"),
            BrowserStoredValueClass::SensitiveOneShotSecret
        );
        assert_eq!(
            browser_stored_value_class(&format!(
                "aura_{}",
                WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX
            )),
            BrowserStoredValueClass::ShortLivedMetadata
        );
        assert_eq!(
            browser_stored_value_class(&format!(
                "aura_{}",
                WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX
            )),
            BrowserStoredValueClass::RestartMetadata
        );
    }

    #[test]
    fn expiring_browser_records_expire_after_ttl() {
        assert!(!record_is_expired(1_000, 1_500, 500));
        assert!(record_is_expired(1_000, 1_501, 500));
        assert!(!record_is_expired(1_000, 900, 500));
    }

    #[test]
    fn pending_bootstrap_persistence_strips_device_enrollment_code() {
        let pending = PendingAccountBootstrap::new("Alice".to_string())
            .with_device_enrollment_code("secret-code".to_string());

        let persisted = pending_bootstrap_for_persistence(&pending);

        assert_eq!(persisted.nickname_suggestion, "Alice");
        assert!(persisted.device_enrollment_code.is_none());
    }

    #[test]
    fn enrollment_code_persistence_uses_session_storage_not_local_storage() {
        let source = include_str!("storage.rs");
        let persist_start = source
            .find("pub(crate) fn persist_pending_device_enrollment_code")
            .expect("pending enrollment persist function exists");
        let persist_end = source[persist_start..]
            .find("pub(crate) fn clear_pending_device_enrollment_code")
            .map(|offset| persist_start + offset)
            .expect("pending enrollment clear function follows persist");
        let persist_block = &source[persist_start..persist_end];

        assert!(persist_block.contains("WebSessionStorage::required"));
        assert!(!persist_block.contains("WebLocalStorage::required"));
    }

    #[test]
    fn harness_mode_requires_feature_and_session_token() {
        let source = include_str!("storage.rs");
        assert!(source.contains("const HARNESS_TOKEN_QUERY_KEY: &str = \"__aura_harness_token\";"));
        assert!(source.contains("const MIN_HARNESS_TOKEN_LEN: usize = 16;"));
        assert!(source.contains("if !cfg!(feature = \"harness\") {"));
        assert!(source.contains("let token = harness_token()?;"));
        assert!(source.contains("harness_session().is_some()"));
    }
}

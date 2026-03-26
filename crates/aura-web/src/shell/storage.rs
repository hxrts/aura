use aura_app::ui::types::{
    BootstrapRuntimeIdentity, PendingAccountBootstrap, WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX,
    WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX, WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX,
};
use aura_ui::FrontendUiOperation as WebUiOperation;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{log_web_error, WebUiError};

const WEB_STORAGE_PREFIX: &str = "aura_";
const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WebStorageScope {
    prefix: String,
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
}

pub(crate) struct WebLocalStorage {
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
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == HARNESS_INSTANCE_QUERY_KEY && !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub(crate) fn harness_mode_enabled() -> bool {
    harness_instance_id().is_some()
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
    let Some(storage) =
        WebLocalStorage::optional_lookup(WebUiOperation::LoadPendingAccountBootstrap)?
    else {
        return Ok(None);
    };
    storage.load_json(
        storage_key,
        WebUiOperation::LoadPendingAccountBootstrap,
        "WEB_PENDING_BOOTSTRAP_READ_FAILED",
        "WEB_PENDING_BOOTSTRAP_PARSE_FAILED",
        "pending account bootstrap",
        "pending account bootstrap",
    )
}

pub(crate) fn persist_pending_account_bootstrap(
    storage_key: &str,
    pending_bootstrap: &PendingAccountBootstrap,
) -> Result<(), WebUiError> {
    WebLocalStorage::required(WebUiOperation::PersistPendingAccountBootstrap)?.set_json(
        storage_key,
        pending_bootstrap,
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
    let Some(storage) =
        WebLocalStorage::optional_lookup(WebUiOperation::LoadPendingDeviceEnrollmentCode)?
    else {
        return Ok(None);
    };
    storage.load_string(
        storage_key,
        WebUiOperation::LoadPendingDeviceEnrollmentCode,
        "WEB_PENDING_DEVICE_ENROLLMENT_CODE_READ_FAILED",
        "pending device enrollment code",
    )
}

pub(crate) fn persist_pending_device_enrollment_code(
    storage_key: &str,
    code: &str,
) -> Result<(), WebUiError> {
    WebLocalStorage::required(WebUiOperation::PersistPendingDeviceEnrollmentCode)?.set_string(
        storage_key,
        code,
        WebUiOperation::PersistPendingDeviceEnrollmentCode,
        "WEB_PENDING_ENROLLMENT_PERSIST_FAILED",
        "pending device enrollment code",
    )
}

pub(crate) fn clear_pending_device_enrollment_code(storage_key: &str) -> Result<(), WebUiError> {
    WebLocalStorage::required(WebUiOperation::ClearPendingDeviceEnrollmentCode)?.remove(
        storage_key,
        WebUiOperation::ClearPendingDeviceEnrollmentCode,
        "WEB_PENDING_ENROLLMENT_CLEAR_FAILED",
        "pending device enrollment code",
    )
}

//! Aura web application entry point for WASM targets.
//!
//! Initializes the Dioxus-based web UI with the AppCore, clipboard adapter,
//! and harness bridge for browser-based deployment and testing.

#![allow(missing_docs)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod error;
        mod harness_bridge;
        mod task_owner;
        mod web_clipboard;

        use async_lock::RwLock;
        use aura_agent::AgentBuilder;
        use aura_app::{AppConfig, AppCore};
        use aura_app::ui::workflows::account as account_workflows;
        use aura_app::ui::workflows::invitation as invitation_workflows;
        use aura_app::ui::workflows::network as network_workflows;
        use aura_app::ui::workflows::runtime as runtime_workflows;
        use aura_app::ui::workflows::settings as settings_workflows;
        use aura_app::ui::workflows::time as time_workflows;
        use aura_app::ui::types::{
            BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity, BootstrapSurface,
            InvitationBridgeType, PendingAccountBootstrap,
            WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX,
            WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX,
        };
        use aura_core::{types::identifiers::AuthorityId, DeviceId};
        use aura_app::ui::contract::{
            ControlId, FieldId, ScreenId, UiReadiness,
        };
        use aura_effects::{new_authority_id, new_device_id, RealRandomHandler};
        use aura_ui::{AuraUiRoot, UiController};
        use dioxus::dioxus_core::schedule_update;
        use dioxus::prelude::*;
        use error::{log_web_error, WebUiError, WebUiOperation};
        use futures::{
            channel::{mpsc, oneshot},
            StreamExt,
        };
        use js_sys::Promise;
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::Arc;
        use task_owner::shared_web_task_owner;
        use wasm_bindgen::JsValue;
        use wasm_bindgen_futures::future_to_promise;
        use web_clipboard::WebClipboardAdapter;

        const WEB_STORAGE_PREFIX: &str = "aura_";
        const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";
        const LEGACY_SELECTED_AUTHORITY_STORAGE_SUFFIX: &str = "selected_authority";
        const LEGACY_SELECTED_DEVICE_STORAGE_SUFFIX: &str = "selected_device";

        fn selected_runtime_identity_key(storage_prefix: &str) -> String {
            format!(
                "{storage_prefix}{}",
                WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX
            )
        }

        fn legacy_selected_authority_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}{LEGACY_SELECTED_AUTHORITY_STORAGE_SUFFIX}")
        }

        fn legacy_selected_device_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}{LEGACY_SELECTED_DEVICE_STORAGE_SUFFIX}")
        }

        fn pending_device_enrollment_code_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}pending_device_enrollment_code")
        }

        fn pending_account_bootstrap_key(storage_prefix: &str) -> String {
            format!(
                "{storage_prefix}{}",
                WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX
            )
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

        fn harness_instance_id() -> Option<String> {
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

        fn harness_mode_enabled() -> bool {
            harness_instance_id().is_some()
        }

        fn active_storage_prefix() -> String {
            if let Some(instance_id) = harness_instance_id() {
                let sanitized = sanitize_storage_segment(&instance_id);
                if !sanitized.is_empty() {
                    return format!("{WEB_STORAGE_PREFIX}{sanitized}_");
                }
            }
            WEB_STORAGE_PREFIX.to_string()
        }

        fn logged_optional<T>(result: Result<Option<T>, WebUiError>) -> Option<T> {
            match result {
                Ok(value) => value,
                Err(error) => {
                    log_web_error("warn", &error);
                    None
                }
            }
        }

        fn apply_harness_mode_document_flags() {
            if harness_instance_id().is_none() {
                return;
            }
            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(document) = window.document() else {
                return;
            };
            let Some(root) = document.document_element() else {
                return;
            };
            if let Err(error) = root.set_attribute("data-aura-harness-mode", "1") {
                log_web_error(
                    "warn",
                    &WebUiError::config(
                        WebUiOperation::ApplyHarnessModeDocumentFlags,
                        "WEB_HARNESS_DOCUMENT_FLAG_SET_FAILED",
                        format!("failed to apply harness mode document flag: {error:?}"),
                    ),
                );
            }
        }

        fn load_selected_authority(storage_key: &str) -> Result<Option<AuthorityId>, WebUiError> {
            let Some(window) = web_sys::window() else {
                return Ok(None);
            };
            let Some(storage) = window.local_storage().map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedAuthority,
                    "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
                    format!("failed to access localStorage: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            let Some(raw) = storage.get_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedAuthority,
                    "WEB_SELECTED_AUTHORITY_READ_FAILED",
                    format!("failed to read selected authority: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            raw.parse::<AuthorityId>().map(Some).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedAuthority,
                    "WEB_SELECTED_AUTHORITY_PARSE_FAILED",
                    format!("failed to parse selected authority: {error}"),
                )
            })
        }

        fn load_selected_device(storage_key: &str) -> Result<Option<DeviceId>, WebUiError> {
            let Some(window) = web_sys::window() else {
                return Ok(None);
            };
            let Some(storage) = window.local_storage().map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedDevice,
                    "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
                    format!("failed to access localStorage: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            let Some(raw) = storage.get_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedDevice,
                    "WEB_SELECTED_DEVICE_READ_FAILED",
                    format!("failed to read selected device: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            raw.parse::<DeviceId>().map(Some).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedDevice,
                    "WEB_SELECTED_DEVICE_PARSE_FAILED",
                    format!("failed to parse selected device: {error}"),
                )
            })
        }

        fn load_selected_runtime_identity(
            storage_key: &str,
            legacy_authority_key: &str,
            legacy_device_key: &str,
        ) -> Result<Option<BootstrapRuntimeIdentity>, WebUiError> {
            let Some(window) = web_sys::window() else {
                return Ok(None);
            };
            let Some(storage) = window.local_storage().map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedRuntimeIdentity,
                    "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
                    format!("failed to access localStorage: {:?}", error),
                )
            })? else {
                return Ok(None);
            };

            if let Some(raw) = storage.get_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadSelectedRuntimeIdentity,
                    "WEB_RUNTIME_IDENTITY_READ_FAILED",
                    format!("failed to read selected runtime identity: {:?}", error),
                )
            })? {
                let identity =
                    serde_json::from_str::<BootstrapRuntimeIdentity>(&raw).map_err(|error| {
                        WebUiError::config(
                            WebUiOperation::LoadSelectedRuntimeIdentity,
                            "WEB_RUNTIME_IDENTITY_PARSE_FAILED",
                            format!("failed to parse selected runtime identity: {error}"),
                        )
                    })?;
                return Ok(Some(identity));
            }

            let authority_id = load_selected_authority(legacy_authority_key)?;
            let device_id = load_selected_device(legacy_device_key)?;
            match (authority_id, device_id) {
                (Some(authority_id), Some(device_id)) => {
                    Ok(Some(BootstrapRuntimeIdentity::new(authority_id, device_id)))
                }
                (None, None) => Ok(None),
                _ => Err(WebUiError::config(
                    WebUiOperation::LoadSelectedRuntimeIdentity,
                    "WEB_RUNTIME_IDENTITY_LEGACY_PARTIAL_STATE",
                    "legacy selected authority/device state is incomplete",
                )),
            }
        }

        fn persist_selected_runtime_identity(
            storage_key: &str,
            identity: &BootstrapRuntimeIdentity,
        ) -> Result<(), WebUiError> {
            let window = web_sys::window().ok_or_else(|| {
                WebUiError::config(
                    WebUiOperation::PersistSelectedRuntimeIdentity,
                    "WEB_WINDOW_UNAVAILABLE",
                    "window is not available",
                )
            })?;
            let storage = window
                .local_storage()
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::PersistSelectedRuntimeIdentity,
                        "WEB_LOCAL_STORAGE_UNAVAILABLE",
                        format!("localStorage unavailable: {:?}", error),
                    )
                })?
                .ok_or_else(|| {
                    WebUiError::config(
                        WebUiOperation::PersistSelectedRuntimeIdentity,
                        "WEB_LOCAL_STORAGE_MISSING",
                        "localStorage unavailable",
                    )
                })?;
            let raw = serde_json::to_string(identity).map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::PersistSelectedRuntimeIdentity,
                    "WEB_RUNTIME_IDENTITY_SERIALIZE_FAILED",
                    format!("failed to serialize runtime identity: {error}"),
                )
            })?;
            storage
                .set_item(storage_key, &raw)
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::PersistSelectedRuntimeIdentity,
                        "WEB_RUNTIME_IDENTITY_PERSIST_FAILED",
                        format!("failed to persist selected runtime identity: {:?}", error),
                    )
                })
        }

        fn clear_storage_key(storage_key: &str) -> Result<(), WebUiError> {
            let window = web_sys::window().ok_or_else(|| {
                WebUiError::config(
                    WebUiOperation::ClearStorageKey,
                    "WEB_WINDOW_UNAVAILABLE",
                    "window is not available",
                )
            })?;
            let storage = window
                .local_storage()
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::ClearStorageKey,
                        "WEB_LOCAL_STORAGE_UNAVAILABLE",
                        format!("localStorage unavailable: {:?}", error),
                    )
                })?
                .ok_or_else(|| {
                    WebUiError::config(
                        WebUiOperation::ClearStorageKey,
                        "WEB_LOCAL_STORAGE_MISSING",
                        "localStorage unavailable",
                    )
                })?;
            storage.remove_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::ClearStorageKey,
                    "WEB_STORAGE_CLEAR_FAILED",
                    format!("failed to clear localStorage key {storage_key}: {:?}", error),
                )
            })
        }

        fn load_pending_account_bootstrap(
            storage_key: &str,
        ) -> Result<Option<PendingAccountBootstrap>, WebUiError> {
            let Some(window) = web_sys::window() else {
                return Ok(None);
            };
            let Some(storage) = window.local_storage().map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadPendingAccountBootstrap,
                    "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
                    format!("failed to access localStorage: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            let Some(raw) = storage.get_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadPendingAccountBootstrap,
                    "WEB_PENDING_BOOTSTRAP_READ_FAILED",
                    format!("failed to read pending account bootstrap: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            serde_json::from_str(&raw).map(Some).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadPendingAccountBootstrap,
                    "WEB_PENDING_BOOTSTRAP_PARSE_FAILED",
                    format!("failed to parse pending account bootstrap: {error}"),
                )
            })
        }

        fn persist_pending_account_bootstrap(
            storage_key: &str,
            pending_bootstrap: &PendingAccountBootstrap,
        ) -> Result<(), WebUiError> {
            let window = web_sys::window().ok_or_else(|| {
                WebUiError::config(
                    WebUiOperation::PersistPendingAccountBootstrap,
                    "WEB_WINDOW_UNAVAILABLE",
                    "window is not available",
                )
            })?;
            let storage = window
                .local_storage()
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::PersistPendingAccountBootstrap,
                        "WEB_LOCAL_STORAGE_UNAVAILABLE",
                        format!("localStorage unavailable: {:?}", error),
                    )
                })?
                .ok_or_else(|| {
                    WebUiError::config(
                        WebUiOperation::PersistPendingAccountBootstrap,
                        "WEB_LOCAL_STORAGE_MISSING",
                        "localStorage unavailable",
                    )
                })?;
            let raw = serde_json::to_string(pending_bootstrap).map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::PersistPendingAccountBootstrap,
                    "WEB_PENDING_BOOTSTRAP_SERIALIZE_FAILED",
                    format!("failed to serialize pending account bootstrap: {error}"),
                )
            })?;
            storage
                .set_item(storage_key, &raw)
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::PersistPendingAccountBootstrap,
                        "WEB_PENDING_BOOTSTRAP_PERSIST_FAILED",
                        format!("failed to persist pending account bootstrap: {:?}", error),
                    )
                })
        }

        async fn stage_initial_web_account_bootstrap(nickname: &str) -> Result<(), WebUiError> {
            let pending_bootstrap = account_workflows::prepare_pending_account_bootstrap(nickname)
                .map_err(|error| {
                    WebUiError::input(
                        WebUiOperation::StageInitialAccountBootstrap,
                        "WEB_PENDING_BOOTSTRAP_PREPARE_FAILED",
                        error.to_string(),
                    )
                })?;
            let storage_prefix = active_storage_prefix();
            let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
            let pending_account_key = pending_account_bootstrap_key(&storage_prefix);
            let random = RealRandomHandler::new();
            let authority_id = new_authority_id(&random).await;
            let device_id = new_device_id(&random).await;
            let runtime_identity = BootstrapRuntimeIdentity::new(authority_id, device_id);

            persist_selected_runtime_identity(&runtime_identity_key, &runtime_identity)?;
            persist_pending_account_bootstrap(&pending_account_key, &pending_bootstrap)?;

            let staged_event = BootstrapEvent::new(
                BootstrapSurface::Web,
                BootstrapEventKind::PendingBootstrapStaged,
            );
            web_sys::console::log_1(&staged_event.to_string().into());
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] staged_initial_account authority={authority_id};device={device_id};nickname={}",
                    pending_bootstrap.nickname_suggestion
                )
                .into(),
            );
            Ok(())
        }

        fn install_harness_instrumentation(controller: Arc<UiController>) {
            if !harness_mode_enabled() {
                return;
            }
            controller.set_ui_snapshot_sink(Arc::new(|snapshot| {
                harness_bridge::publish_ui_snapshot(&snapshot);
            }));

            harness_bridge::set_controller(controller.clone());
            if let Err(error) = harness_bridge::install_window_harness_api() {
                log_web_error(
                    "error",
                    &WebUiError::operation(
                        WebUiOperation::InstallHarnessInstrumentation,
                        "WEB_HARNESS_API_INSTALL_FAILED",
                        format!("failed to install harness API: {error:?}"),
                    ),
                );
            }
        }

        fn load_pending_device_enrollment_code(
            storage_key: &str,
        ) -> Result<Option<String>, WebUiError> {
            let Some(window) = web_sys::window() else {
                return Ok(None);
            };
            let Some(storage) = window.local_storage().map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadPendingDeviceEnrollmentCode,
                    "WEB_LOCAL_STORAGE_LOOKUP_FAILED",
                    format!("failed to access localStorage: {:?}", error),
                )
            })? else {
                return Ok(None);
            };
            storage.get_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::LoadPendingDeviceEnrollmentCode,
                    "WEB_PENDING_DEVICE_ENROLLMENT_CODE_READ_FAILED",
                    format!("failed to read pending device enrollment code: {:?}", error),
                )
            })
        }

        fn persist_pending_device_enrollment_code(
            storage_key: &str,
            code: &str,
        ) -> Result<(), WebUiError> {
            let window = web_sys::window().ok_or_else(|| {
                WebUiError::config(
                    WebUiOperation::PersistPendingDeviceEnrollmentCode,
                    "WEB_WINDOW_UNAVAILABLE",
                    "window is not available",
                )
            })?;
            let storage = window
                .local_storage()
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::PersistPendingDeviceEnrollmentCode,
                        "WEB_LOCAL_STORAGE_UNAVAILABLE",
                        format!("localStorage unavailable: {:?}", error),
                    )
                })?
                .ok_or_else(|| {
                    WebUiError::config(
                        WebUiOperation::PersistPendingDeviceEnrollmentCode,
                        "WEB_LOCAL_STORAGE_MISSING",
                        "localStorage unavailable",
                    )
                })?;
            storage.set_item(storage_key, code).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::PersistPendingDeviceEnrollmentCode,
                    "WEB_PENDING_ENROLLMENT_PERSIST_FAILED",
                    format!("failed to persist pending device enrollment code: {:?}", error),
                )
            })
        }

        fn clear_pending_device_enrollment_code(storage_key: &str) -> Result<(), WebUiError> {
            let window = web_sys::window().ok_or_else(|| {
                WebUiError::config(
                    WebUiOperation::ClearPendingDeviceEnrollmentCode,
                    "WEB_WINDOW_UNAVAILABLE",
                    "window is not available",
                )
            })?;
            let storage = window
                .local_storage()
                .map_err(|error| {
                    WebUiError::config(
                        WebUiOperation::ClearPendingDeviceEnrollmentCode,
                        "WEB_LOCAL_STORAGE_UNAVAILABLE",
                        format!("localStorage unavailable: {:?}", error),
                    )
                })?
                .ok_or_else(|| {
                    WebUiError::config(
                        WebUiOperation::ClearPendingDeviceEnrollmentCode,
                        "WEB_LOCAL_STORAGE_MISSING",
                        "localStorage unavailable",
                    )
                })?;
            storage.remove_item(storage_key).map_err(|error| {
                WebUiError::config(
                    WebUiOperation::ClearPendingDeviceEnrollmentCode,
                    "WEB_PENDING_ENROLLMENT_CLEAR_FAILED",
                    format!("failed to clear pending device enrollment code: {:?}", error),
                )
            })
        }

        async fn request_runtime_rebootstrap() -> Result<(), WebUiError> {
            harness_bridge::request_rebootstrap().await.map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::ReloadPage,
                    "WEB_REBOOTSTRAP_REQUEST_FAILED",
                    format!("failed to request in-process rebootstrap: {:?}", error),
                )
            })
        }

        #[derive(Clone, PartialEq)]
        struct BootstrapState {
            controller: Arc<UiController>,
            account_ready: bool,
        }

        async fn bootstrap_controller() -> Result<BootstrapState, WebUiError> {
            let storage_prefix = active_storage_prefix();
            let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
            let legacy_authority_key = legacy_selected_authority_key(&storage_prefix);
            let legacy_device_key = legacy_selected_device_key(&storage_prefix);
            let pending_account_key = pending_account_bootstrap_key(&storage_prefix);
            let selected_runtime_identity = logged_optional(load_selected_runtime_identity(
                &runtime_identity_key,
                &legacy_authority_key,
                &legacy_device_key,
            ));
            let pending_account_bootstrap =
                logged_optional(load_pending_account_bootstrap(&pending_account_key));
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] storage_prefix={storage_prefix};selected_runtime_identity={:?};pending_account_bootstrap={:?}",
                    selected_runtime_identity, pending_account_bootstrap
                )
                .into(),
            );
            let harness_instance = harness_instance_id();
            let clipboard = Arc::new(WebClipboardAdapter::default());
            if let Some(runtime_identity) = selected_runtime_identity {
                let authority_id = runtime_identity.authority_id;
                let device_id = runtime_identity.device_id;
                let config = aura_agent::core::AgentConfig {
                    device_id,
                    ..Default::default()
                };
                let agent = Arc::new(
                    AgentBuilder::web()
                        .storage_prefix(&storage_prefix)
                        .authority(authority_id)
                        .with_config(config)
                        .build()
                        .await
                        .map_err(|error| {
                            WebUiError::operation(
                                WebUiOperation::BootstrapController,
                                "WEB_RUNTIME_BUILD_FAILED",
                                format!("failed to build web runtime: {error}"),
                            )
                        })?,
                );
                web_sys::console::log_1(
                    &format!(
                        "[web-bootstrap] runtime_authority={};runtime_device={}",
                        agent.authority_id(),
                        agent.runtime().device_id()
                    )
                    .into(),
                );

                let app_core = Arc::new(RwLock::new(
                    AppCore::with_runtime(AppConfig::default(), agent.clone().as_runtime_bridge())
                        .map_err(|error| {
                            WebUiError::operation(
                                WebUiOperation::BootstrapController,
                                "WEB_APP_CORE_INIT_FAILED",
                                format!("failed to initialize AppCore: {error}"),
                            )
                        })?,
                ));

                let ceremony_agent = agent.clone();
                let ceremony_app_core = app_core.clone();
                shared_web_task_owner().spawn_local_cancellable(async move {
                    loop {
                        if let Err(error) =
                            time_workflows::sleep_ms(&ceremony_app_core, 500).await
                        {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::ProcessCeremonyAcceptances,
                                    "WEB_CEREMONY_ACCEPTANCE_SLEEP_FAILED",
                                    error.to_string(),
                                ),
                            );
                            break;
                        }
                        if let Err(error) = ceremony_agent.process_ceremony_acceptances().await {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::ProcessCeremonyAcceptances,
                                    "WEB_CEREMONY_ACCEPTANCE_PROCESS_FAILED",
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                });

                AppCore::init_signals_with_hooks(&app_core)
                    .await
                    .map_err(|error| {
                        WebUiError::operation(
                            WebUiOperation::BootstrapController,
                            "WEB_SIGNAL_INIT_FAILED",
                            format!("failed to initialize app signals: {error}"),
                        )
                    })?;

                let bootstrap_resolution =
                    account_workflows::reconcile_pending_runtime_account_bootstrap(
                        &app_core,
                        pending_account_bootstrap.clone(),
                    )
                    .await
                    .map_err(|error| {
                        WebUiError::operation(
                            WebUiOperation::BootstrapController,
                            "WEB_PENDING_BOOTSTRAP_RECONCILE_FAILED",
                            format!("failed to reconcile pending web account bootstrap: {error}"),
                        )
                    })?;
                let account_ready = bootstrap_resolution.account_ready;

                if bootstrap_resolution.action
                    != account_workflows::PendingRuntimeBootstrapAction::None
                {
                    let reconciled_event = BootstrapEvent::new(
                        BootstrapSurface::Web,
                        BootstrapEventKind::PendingBootstrapReconciled,
                    );
                    web_sys::console::log_1(&reconciled_event.to_string().into());
                    clear_storage_key(&pending_account_key)?;
                }

                let current_runtime_identity = BootstrapRuntimeIdentity::new(
                    agent.authority_id().clone(),
                    agent.runtime().device_id(),
                );
                if let Err(error) = persist_selected_runtime_identity(
                    &runtime_identity_key,
                    &current_runtime_identity,
                ) {
                    log_web_error("warn", &error);
                }
                if let Err(error) = clear_storage_key(&legacy_authority_key) {
                    log_web_error("warn", &error);
                }
                if let Err(error) = clear_storage_key(&legacy_device_key) {
                    log_web_error("warn", &error);
                }

                let current_device_id = current_runtime_identity.device_id;
                let controller = Arc::new(UiController::with_authority_switcher(
                    app_core,
                    clipboard,
                    Some(Arc::new(move |authority_id: AuthorityId| {
                        let storage_prefix = active_storage_prefix();
                        let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
                        let legacy_authority_key = legacy_selected_authority_key(&storage_prefix);
                        let legacy_device_key = legacy_selected_device_key(&storage_prefix);
                        let runtime_identity = load_selected_runtime_identity(
                            &runtime_identity_key,
                            &legacy_authority_key,
                            &legacy_device_key,
                        );
                        let runtime_identity =
                            logged_optional(runtime_identity).unwrap_or_else(|| {
                                BootstrapRuntimeIdentity::new(
                                    authority_id,
                                    current_device_id.clone(),
                                )
                            });
                        let updated_identity = BootstrapRuntimeIdentity::new(
                            authority_id,
                            runtime_identity.device_id,
                        );
                        if let Err(error) = persist_selected_runtime_identity(
                            &runtime_identity_key,
                            &updated_identity,
                        ) {
                            log_web_error("error", &error);
                            return;
                        }
                        if let Err(error) = clear_storage_key(&legacy_authority_key) {
                            log_web_error("warn", &error);
                        }
                        if let Err(error) = clear_storage_key(&legacy_device_key) {
                            log_web_error("warn", &error);
                        }
                        shared_web_task_owner().spawn_local(async move {
                            if let Err(error) = request_runtime_rebootstrap().await {
                                log_web_error("error", &error);
                            }
                        });
                    })),
                ));
                install_harness_instrumentation(controller.clone());
                controller.set_account_setup_state(account_ready, "", None);

                if account_ready {
                    if let Err(error) = settings_workflows::refresh_settings_from_runtime(
                        controller.app_core(),
                    )
                    .await
                    {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::RefreshBootstrapSettings,
                                "WEB_BOOTSTRAP_SETTINGS_REFRESH_FAILED",
                                error.to_string(),
                            ),
                        );
                    }
                    match runtime_workflows::require_runtime(controller.app_core()).await {
                        Ok(runtime) => {
                            let runtime_devices = runtime
                                .try_list_devices()
                                .await
                                .unwrap_or_default();
                            match settings_workflows::get_settings(controller.app_core()).await {
                                Ok(settings) => web_sys::console::log_1(
                                    &format!(
                                        "[web-bootstrap] settings_seeded runtime_devices={:?};settings_devices={:?}",
                                        runtime_devices
                                            .iter()
                                            .map(|device| device.id.to_string())
                                            .collect::<Vec<_>>(),
                                        settings
                                            .devices
                                            .iter()
                                            .map(|device| device.id.clone())
                                            .collect::<Vec<_>>()
                                    )
                                    .into(),
                                ),
                                Err(error) => log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::InspectBootstrapRuntime,
                                        "WEB_BOOTSTRAP_SETTINGS_INSPECT_FAILED",
                                        format!(
                                            "failed to inspect seeded settings for runtime devices {:?}: {error}",
                                            runtime_devices
                                                .iter()
                                                .map(|device| device.id.to_string())
                                                .collect::<Vec<_>>()
                                        ),
                                    ),
                                ),
                            }
                        }
                        Err(error) => log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::InspectBootstrapRuntime,
                                "WEB_BOOTSTRAP_RUNTIME_INSPECT_FAILED",
                                error.to_string(),
                            ),
                        ),
                    }
                }

                let finalized_event = BootstrapEvent::new(
                    BootstrapSurface::Web,
                    BootstrapEventKind::RuntimeBootstrapFinalized,
                );
                controller.push_log(&finalized_event.to_string());
                if let Some(instance_id) = harness_instance {
                    controller.push_log(&format!(
                        "web harness instance {instance_id} booted in testing mode"
                    ));
                }
                Ok(BootstrapState {
                    controller,
                    account_ready,
                })
            } else {
                if logged_optional(load_selected_authority(&legacy_authority_key)).is_some()
                    || logged_optional(load_selected_device(&legacy_device_key)).is_some()
                {
                    log_web_error(
                        "warn",
                        &WebUiError::config(
                            WebUiOperation::BootstrapController,
                            "WEB_BOOTSTRAP_LEGACY_IDENTITY_INCOMPLETE",
                            "incomplete persisted bootstrap identity; starting without runtime",
                        ),
                    );
                }
                let app_core = Arc::new(RwLock::new(
                    AppCore::new(AppConfig::default())
                        .map_err(|error| {
                            WebUiError::operation(
                                WebUiOperation::BootstrapController,
                                "WEB_BOOTSTRAP_APP_CORE_INIT_FAILED",
                                format!("failed to initialize bootstrap AppCore: {error}"),
                            )
                        })?,
                ));
                AppCore::init_signals_with_hooks(&app_core)
                    .await
                    .map_err(|error| {
                        WebUiError::operation(
                            WebUiOperation::BootstrapController,
                            "WEB_BOOTSTRAP_SIGNAL_INIT_FAILED",
                            format!("failed to initialize bootstrap app signals: {error}"),
                        )
                    })?;
                let controller = Arc::new(UiController::new(app_core, clipboard));
                install_harness_instrumentation(controller.clone());
                controller.set_account_setup_state(false, "", None);
                let waiting_event = BootstrapEvent::new(
                    BootstrapSurface::Web,
                    BootstrapEventKind::ShellAwaitingAccount,
                );
                controller.push_log(&waiting_event.to_string());
                if let Some(instance_id) = harness_instance {
                    controller.push_log(&format!(
                        "web harness instance {instance_id} booted without runtime"
                    ));
                }
                Ok(BootstrapState {
                    controller,
                    account_ready: false,
                })
            }
        }

        fn main() {
            aura_app::platform::wasm::initialize();
            apply_harness_mode_document_flags();
            let mut tracing_config = tracing_wasm::WASMLayerConfigBuilder::new();
            tracing_config
                .set_max_level(tracing::Level::INFO)
                .set_report_logs_in_timings(false);
            tracing_wasm::set_as_global_default_with_config(tracing_config.build());
            dioxus::launch(App);
        }

        #[component]
        fn App() -> Element {
            let bootstrap_started = use_hook(|| Rc::new(Cell::new(false)));
            let bootstrap_epoch = use_signal(|| 0_u64);
            let committed_bootstrap = use_signal(|| Option::<BootstrapState>::None);
            let bootstrap_error = use_signal(|| Option::<WebUiError>::None);
            let rebootstrap_requests = use_hook(|| {
                let (tx, rx) = mpsc::channel::<oneshot::Sender<Result<(), WebUiError>>>(4);
                (
                    tx,
                    Rc::new(std::cell::RefCell::new(Some(rx))),
                )
            });

            use_effect(|| {
                if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                    document.set_title("Aura");
                }
            });

            use_effect(move || {
                if !bootstrap_started.get() {
                    bootstrap_started.set(true);
                    let mut rebootstrap_rx = rebootstrap_requests
                        .1
                        .borrow_mut()
                        .take()
                        .expect("rebootstrap receiver should initialize once");
                    let mut bootstrap_epoch = bootstrap_epoch;
                    let mut committed_bootstrap = committed_bootstrap;
                    let mut bootstrap_error = bootstrap_error;
                    shared_web_task_owner().spawn_local_cancellable(async move {
                        while let Some(completion_tx) = rebootstrap_rx.next().await {
                            let epoch = bootstrap_epoch() + 1;
                            web_sys::console::log_1(
                                &format!("[web-bootstrap] trigger start epoch={epoch}").into(),
                            );
                            bootstrap_epoch.set(epoch);

                            web_sys::console::log_1(
                                &format!("[web-bootstrap] runner start epoch={epoch}").into(),
                            );
                            let result = bootstrap_controller().await;
                            if bootstrap_epoch() != epoch {
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-bootstrap] runner superseded epoch={epoch};current={}",
                                        bootstrap_epoch()
                                    )
                                    .into(),
                                );
                                let _ = completion_tx.send(Ok(()));
                                continue;
                            }
                            match result {
                                Ok(state) => {
                                    web_sys::console::log_1(
                                        &format!("[web-bootstrap] runner ok epoch={epoch}").into(),
                                    );
                                    bootstrap_error.set(None);
                                    committed_bootstrap.set(Some(state));
                                    let _ = completion_tx.send(Ok(()));
                                }
                                Err(error) => {
                                    web_sys::console::error_1(
                                        &format!(
                                            "[web-bootstrap] runner error epoch={epoch} error={}",
                                            error.user_message()
                                        )
                                        .into(),
                                    );
                                    if committed_bootstrap().is_none() {
                                        bootstrap_error.set(Some(error.clone()));
                                    } else {
                                        log_web_error("error", &error);
                                    }
                                    let _ = completion_tx.send(Err(error));
                                }
                            }
                        }
                    });

                    let (completion_tx, _completion_rx) =
                        oneshot::channel::<Result<(), WebUiError>>();
                    if rebootstrap_requests.0.clone().try_send(completion_tx).is_err() {
                        web_sys::console::error_1(
                            &"[web-bootstrap] failed to enqueue initial bootstrap request".into(),
                        );
                    }
                }

                let trigger: Arc<dyn Fn() -> js_sys::Promise> = Arc::new({
                    let rebootstrap_tx = rebootstrap_requests.0.clone();
                    move || {
                        let (completion_tx, completion_rx) =
                            oneshot::channel::<Result<(), WebUiError>>();
                        if rebootstrap_tx.clone().try_send(completion_tx).is_err() {
                            return Promise::reject(&JsValue::from_str(
                                "rebootstrap request queue is unavailable or full",
                            ));
                        }

                        future_to_promise(async move {
                            match completion_rx.await {
                                Ok(Ok(())) => {
                                    web_sys::console::log_1(&"[web-bootstrap] trigger done".into());
                                    Ok(JsValue::UNDEFINED)
                                }
                                Ok(Err(error)) => {
                                    web_sys::console::error_1(
                                        &format!(
                                            "[web-bootstrap] trigger failed error={}",
                                            error.user_message()
                                        )
                                        .into(),
                                    );
                                    Err(JsValue::from_str(&error.user_message()))
                                }
                                Err(_) => Err(JsValue::from_str(
                                    "bootstrap completion channel dropped unexpectedly",
                                )),
                            }
                        })
                    }
                });

                harness_bridge::set_rebootstrap_trigger(trigger);
            });

            if let Some(state) = committed_bootstrap() {
                return rsx! {
                    BootstrappedApp {
                        state,
                    }
                };
            }

            if let Some(error) = bootstrap_error() {
                return rsx! {
                    main {
                        class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                        div {
                            class: "max-w-xl space-y-3 text-center",
                            h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                            p { class: "text-sm text-muted-foreground", "Web runtime bootstrap failed." }
                            p { class: "text-xs text-muted-foreground break-words", "{error.user_message()}" }
                        }
                    }
                };
            }

            rsx! {
                main {
                    class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                    div {
                        class: "max-w-xl space-y-3 text-center",
                        h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                        p { class: "text-sm text-muted-foreground", "Initializing web runtime..." }
                    }
                }
            }
        }

        #[component]
        fn BootstrappedApp(state: BootstrapState) -> Element {
            let controller = state.controller.clone();
            let rerender = schedule_update();
            controller.set_rerender_callback(rerender.clone());
            let mut sync_loop_started = use_signal(|| false);
            let bootstrap_account_ready = use_signal(|| state.account_ready);
            let mut account_name = use_signal(String::new);
            let mut account_error = use_signal(|| Option::<WebUiError>::None);
            let creating_account = use_signal(|| false);
            let mut import_code = use_signal(String::new);
            let mut import_error = use_signal(|| Option::<WebUiError>::None);
            let importing_code = use_signal(|| false);
            let mut auto_import_started = use_signal(|| false);
            let controller_snapshot = controller.semantic_model_snapshot();
            let controller_account_ready = controller_snapshot.readiness == UiReadiness::Ready
                && controller_snapshot.screen != ScreenId::Onboarding;
            let account_ready = bootstrap_account_ready() || controller_account_ready;

            if account_ready && !sync_loop_started() {
                sync_loop_started.set(true);
                let app_core = controller.app_core().clone();
                shared_web_task_owner().spawn_local_cancellable(async move {
                    loop {
                        let runtime = { app_core.read().await.runtime().cloned() };
                        if let Some(runtime) = runtime {
                            if let Err(error) = runtime.trigger_discovery().await {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_DISCOVERY_TRIGGER_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                            if let Err(error) = runtime.trigger_sync().await {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_SYNC_TRIGGER_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                        }
                        if let Err(error) =
                            network_workflows::refresh_discovered_peers(&app_core).await
                        {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::BackgroundSync,
                                    "WEB_DISCOVERED_PEERS_REFRESH_FAILED",
                                    error.to_string(),
                                ),
                            );
                        }
                        if let Err(error) = time_workflows::sleep_ms(&app_core, 1_500).await {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::BackgroundSync,
                                    "WEB_BACKGROUND_SYNC_SLEEP_FAILED",
                                    error.to_string(),
                                ),
                            );
                            break;
                        }
                    }
                });
            }

            if account_ready {
                return rsx! {
                    AuraUiRoot {
                        controller: controller.clone(),
                    }
                };
            }

            let run_import: Arc<dyn Fn(String)> = Arc::new({
                let controller = controller.clone();
                let import_error = import_error.clone();
                let importing_code = importing_code.clone();
                let mut bootstrap_account_ready = bootstrap_account_ready.clone();
                move |code: String| {
                    let mut import_error = import_error.clone();
                    let mut importing_code = importing_code.clone();
                    if importing_code() {
                        return;
                    }

                    let storage_prefix = active_storage_prefix();
                    let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
                    let legacy_authority_key = legacy_selected_authority_key(&storage_prefix);
                    let legacy_device_key = legacy_selected_device_key(&storage_prefix);
                    let pending_code_storage_key =
                        pending_device_enrollment_code_key(&storage_prefix);
                    importing_code.set(true);
                    import_error.set(None);

                    let controller = controller.clone();
                    shared_web_task_owner().spawn_local(async move {
                        let app_core = controller.app_core().clone();
                        let result = async {
                            let mut requires_rebootstrap = false;
                            let invitation = invitation_workflows::import_invitation_details(
                                &app_core, &code,
                            )
                            .await
                            .map_err(|error| {
                                WebUiError::operation(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_DEVICE_ENROLLMENT_IMPORT_DETAILS_FAILED",
                                        error.to_string(),
                                    )
                                })?;
                            let invitation_info = invitation.info().clone();
                            let InvitationBridgeType::DeviceEnrollment {
                                subject_authority,
                                device_id,
                                ..
                            } = invitation_info.invitation_type.clone()
                            else {
                                return Err(WebUiError::input(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_DEVICE_ENROLLMENT_CODE_INVALID_KIND",
                                    "Code is not a device enrollment invitation",
                                ));
                            };

                            let runtime = runtime_workflows::require_runtime(&app_core)
                                .await
                                .map_err(|error| {
                                    WebUiError::operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                        "WEB_RUNTIME_REQUIRED_FAILED",
                                        error.to_string(),
                                    )
                                })?;
                            let current_authority = runtime.authority_id();
                            let selected_runtime_identity = logged_optional(
                                load_selected_runtime_identity(
                                    &runtime_identity_key,
                                    &legacy_authority_key,
                                    &legacy_device_key,
                                ),
                            );
                            if current_authority != subject_authority
                                || selected_runtime_identity
                                    .as_ref()
                                    .map(|identity| identity.device_id)
                                    != Some(device_id)
                            {
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] staging_rebootstrap current_authority={};subject_authority={};selected_runtime_identity={:?};invited_device={}",
                                        current_authority,
                                        subject_authority,
                                        selected_runtime_identity,
                                        device_id
                                    )
                                    .into(),
                                );
                                persist_pending_device_enrollment_code(
                                    &pending_code_storage_key,
                                    &code,
                                )
                                .map_err(|error| {
                                    error.with_operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                    )
                                })?;
                                persist_selected_runtime_identity(
                                    &runtime_identity_key,
                                    &BootstrapRuntimeIdentity::new(subject_authority, device_id),
                                )
                                .map_err(|error| {
                                    error.with_operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                    )
                                })?;
                                if let Err(error) = clear_storage_key(&legacy_authority_key) {
                                    log_web_error("warn", &error);
                                }
                                if let Err(error) = clear_storage_key(&legacy_device_key) {
                                    log_web_error("warn", &error);
                                }
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] staged_rebootstrap subject_authority={};device_id={}",
                                        subject_authority, device_id
                                    )
                                    .into(),
                                );
                                requires_rebootstrap = true;
                                request_runtime_rebootstrap().await.map_err(|error| {
                                    error.with_operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                    )
                                })?;
                                return Ok(requires_rebootstrap);
                            }

                            if let Err(error) =
                                clear_pending_device_enrollment_code(&pending_code_storage_key)
                            {
                                log_web_error("warn", &error);
                            }
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepting_on_bound_runtime authority={};selected_runtime_identity={:?};invited_device={}",
                                    current_authority,
                                    selected_runtime_identity,
                                    device_id
                                )
                                .into(),
                            );

                            for _ in 0..8 {
                                runtime_workflows::converge_runtime(&runtime).await;
                                if runtime_workflows::ensure_runtime_peer_connectivity(
                                    &runtime,
                                    "device_enrollment_accept",
                                )
                                .await
                                .is_ok()
                                {
                                    break;
                                }
                                time_workflows::sleep_ms(&app_core, 250)
                                    .await
                                    .map_err(|error| {
                                        WebUiError::operation(
                                            WebUiOperation::ImportDeviceEnrollmentCode,
                                            "WEB_DEVICE_ENROLLMENT_SLEEP_FAILED",
                                            error.to_string(),
                                        )
                                    })?;
                            }

                            invitation_workflows::accept_device_enrollment_invitation(
                                &app_core,
                                &invitation_info,
                            )
                            .await
                            .map_err(|error| {
                                WebUiError::operation(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_DEVICE_ENROLLMENT_ACCEPT_FAILED",
                                    error.to_string(),
                                )
                            })?;
                            let runtime_devices_after_accept = runtime
                                .try_list_devices()
                                .await
                                .unwrap_or_default();
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepted runtime_devices={:?}",
                                    runtime_devices_after_accept
                                        .iter()
                                        .map(|device| device.id.to_string())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            );
                            let settings = settings_workflows::get_settings(&app_core)
                                .await
                                .map_err(|error| {
                                    WebUiError::operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                        "WEB_DEVICE_ENROLLMENT_SETTINGS_FETCH_FAILED",
                                        error.to_string(),
                                    )
                                })?;
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepted settings_devices={:?}",
                                    settings
                                        .devices
                                        .iter()
                                        .map(|device| device.id.clone())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            );
                            let nickname = settings.nickname_suggestion.trim();
                            let bootstrap_name = if nickname.is_empty() {
                                "Aura User".to_string()
                            } else {
                                nickname.to_string()
                            };
                            account_workflows::initialize_runtime_account(
                                &app_core,
                                bootstrap_name,
                            )
                            .await
                            .map_err(|error| {
                                WebUiError::operation(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_DEVICE_ENROLLMENT_ACCOUNT_INIT_FAILED",
                                    error.to_string(),
                                )
                            })?;
                            Ok(requires_rebootstrap)
                        }
                        .await;

                        match result {
                            Ok(requires_rebootstrap) => {
                                if !requires_rebootstrap {
                                    controller.info_toast("Device enrollment complete");
                                    bootstrap_account_ready.set(true);
                                    controller.set_account_setup_state(true, "", None);
                                } else {
                                    controller.info_toast("Switching runtime to finish import");
                                }
                                importing_code.set(false);
                            }
                            Err(error) => {
                                if let Err(clear_error) =
                                    clear_pending_device_enrollment_code(&pending_code_storage_key)
                                {
                                    log_web_error("warn", &clear_error);
                                }
                                let message = error.user_message();
                                controller.set_account_setup_state(
                                    false,
                                    "",
                                    Some(message.clone()),
                                );
                                import_error.set(Some(error));
                                importing_code.set(false);
                            }
                        }
                    });
                }
            });

            let pending_code_storage_key =
                pending_device_enrollment_code_key(&active_storage_prefix());
            if !auto_import_started() {
                if let Some(pending_code) = logged_optional(
                    load_pending_device_enrollment_code(&pending_code_storage_key),
                )
                {
                    if !pending_code.is_empty() {
                        auto_import_started.set(true);
                        import_code.set(pending_code.clone());
                        run_import(pending_code);
                    }
                }
            }

            let submit_account = {
                let controller = controller.clone();
                let account_name = account_name.clone();
                let mut account_error = account_error.clone();
                let mut creating_account = creating_account.clone();
                let mut bootstrap_account_ready = bootstrap_account_ready.clone();
                move |_| {
                    if creating_account() {
                        return;
                    }

                    let nickname = account_name();
                    web_sys::console::log_1(
                        &format!(
                            "[web-onboarding] submit_account start nickname={}",
                            nickname
                        )
                        .into(),
                    );
                    creating_account.set(true);
                    account_error.set(None);
                    controller.set_account_setup_state(false, nickname.clone(), None);

                    let controller = controller.clone();
                    shared_web_task_owner().spawn_local(async move {
                        let has_runtime = {
                            let core = controller.app_core().read().await;
                            core.runtime().is_some()
                        };
                        let result = if has_runtime {
                            account_workflows::initialize_runtime_account(
                                controller.app_core(),
                                nickname.clone(),
                            )
                            .await
                            .map_err(|error| {
                                WebUiError::operation(
                                    WebUiOperation::CreateAccount,
                                    "WEB_CREATE_ACCOUNT_INIT_FAILED",
                                    error.to_string(),
                                )
                            })
                        } else {
                            match stage_initial_web_account_bootstrap(&nickname).await {
                                Ok(()) => request_runtime_rebootstrap().await,
                                Err(error) => Err(error),
                            }
                        };

                        match result {
                            Ok(()) => {
                                web_sys::console::log_1(
                                    &"[web-onboarding] submit_account ok".into(),
                                );
                                if has_runtime {
                                    bootstrap_account_ready.set(true);
                                    controller.set_account_setup_state(true, "", None);
                                } else {
                                    controller.info_toast("Finishing account bootstrap");
                                }
                                creating_account.set(false);
                            }
                            Err(error) => {
                                log_web_error("error", &error);
                                let message = error.user_message();
                                controller.set_account_setup_state(
                                    false,
                                    nickname.clone(),
                                    Some(message.clone()),
                                );
                                account_error.set(Some(error));
                                creating_account.set(false);
                            }
                        }
                    });
                }
            };

            let submit_import = {
                let import_code = import_code.clone();
                let run_import = run_import.clone();
                move |_| {
                    let code = import_code();
                    run_import(code);
                }
            };

            rsx! {
                main {
                    class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                    div {
                        id: ControlId::OnboardingRoot
                            .web_dom_id()
                            .expect("ControlId::OnboardingRoot must define a web DOM id"),
                        class: "grid place-items-center px-6",
                        div {
                            id: "aura-onboarding-card",
                            class: "w-full max-w-md rounded-3xl border border-border bg-card px-6 py-8 shadow-sm",
                            div {
                                class: "space-y-2",
                                h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                                h2 { class: "text-2xl font-semibold", "Welcome to Aura" }
                                p {
                                    class: "text-sm text-muted-foreground",
                                    "Create the local account profile for this browser before entering the app."
                                }
                            }
                            div {
                                class: "mt-6 space-y-4",
                                label {
                                    class: "block space-y-2",
                                    span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Nickname" }
                                    input {
                                    id: FieldId::AccountName
                                        .web_dom_id()
                                        .expect("FieldId::AccountName must define a web DOM id"),
                                        class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                        value: "{account_name()}",
                                        disabled: creating_account(),
                                        oninput: move |event| {
                                            let value = event.value();
                                            account_name.set(value.clone());
                                            account_error.set(None);
                                            controller.set_account_setup_state(false, value, None);
                                        },
                                    }
                                }
                                if let Some(error) = account_error() {
                                    p { class: "text-sm text-destructive", "{error.user_message()}" }
                                }
                                button {
                                    id: ControlId::OnboardingCreateAccountButton
                                        .web_dom_id()
                                        .expect("ControlId::OnboardingCreateAccountButton must define a web DOM id"),
                                    class: "inline-flex h-10 w-full items-center justify-center rounded-md bg-foreground px-4 text-sm font-medium text-background transition-colors disabled:pointer-events-none disabled:opacity-50",
                                    disabled: creating_account() || account_name().trim().is_empty(),
                                    onclick: submit_account,
                                    if creating_account() {
                                        "Creating Account..."
                                    } else {
                                        "Create Account"
                                    }
                                }
                                div { class: "flex items-center gap-3 py-1",
                                    div { class: "h-px flex-1 bg-border" }
                                    span { class: "text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground", "or" }
                                    div { class: "h-px flex-1 bg-border" }
                                }
                                label {
                                    class: "block space-y-2",
                                    span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Device Enrollment Code" }
                                    input {
                                    id: FieldId::DeviceImportCode
                                        .web_dom_id()
                                        .expect("FieldId::DeviceImportCode must define a web DOM id"),
                                        class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                        value: "{import_code()}",
                                        disabled: importing_code(),
                                        oninput: move |event| {
                                            import_code.set(event.value());
                                            import_error.set(None);
                                        },
                                    }
                                }
                                if let Some(error) = import_error() {
                                    p { class: "text-sm text-destructive", "{error.user_message()}" }
                                }
                                button {
                                    id: ControlId::OnboardingImportDeviceButton
                                        .web_dom_id()
                                        .expect("ControlId::OnboardingImportDeviceButton must define a web DOM id"),
                                    class: "inline-flex h-10 w-full items-center justify-center rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors disabled:pointer-events-none disabled:opacity-50",
                                    disabled: importing_code() || import_code().trim().is_empty(),
                                    onclick: submit_import,
                                    if importing_code() {
                                        "Importing Device..."
                                    } else {
                                        "Import Device"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        fn main() {
            eprintln!("aura-web is a wasm32 frontend. Build with target wasm32-unknown-unknown.");
        }
    }
}

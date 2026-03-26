//! Aura web application entry point for WASM targets.
//!
//! Initializes the Dioxus-based web UI with the AppCore, clipboard adapter,
//! and harness bridge for browser-based deployment and testing.

#![allow(missing_docs)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod bootstrap_storage;
        mod error;
        mod harness_bridge;
        mod shell_host;
        mod task_owner;
        mod web_clipboard;
        mod workflows;

        use async_lock::{Mutex, RwLock};
        use aura_app::{AppCore};
        use aura_app::ui::workflows::account as account_workflows;
        use aura_app::ui::workflows::network as network_workflows;
        use aura_app::ui::workflows::runtime as runtime_workflows;
        use aura_app::ui::workflows::system as system_workflows;
        use aura_app::ui::workflows::time as time_workflows;
        use aura_app::ui::types::{
            BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity, BootstrapSurface,
            PendingAccountBootstrap, WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX,
            WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX,
        };
        use aura_app::ui::contract::{
            ControlId, FieldId, ScreenId, UiReadiness,
        };
        use aura_effects::{new_authority_id, new_device_id, RealRandomHandler};
        use aura_ui::{AuraUiRoot, FrontendUiOperation as WebUiOperation, UiController};
        use dioxus::dioxus_core::schedule_update;
        use dioxus::prelude::*;
        use error::{log_web_error, WebUiError};
        use std::cell::Cell;
        use std::future::Future;
        use std::rc::Rc;
        use std::sync::Arc;
        use shell_host::{BootstrapState, WebShellHost};
        use task_owner::shared_web_task_owner;
        use workflows::{
            AccountCreationStageMode, CurrentRuntimeIdentity, DeviceEnrollmentImportRequest,
            RebootstrapPolicy,
        };

        const WEB_STORAGE_PREFIX: &str = "aura_";
        const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";
        pub(crate) fn selected_runtime_identity_key(storage_prefix: &str) -> String {
            format!(
                "{storage_prefix}{}",
                WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX
            )
        }

        pub(crate) fn pending_device_enrollment_code_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}pending_device_enrollment_code")
        }

        pub(crate) fn pending_account_bootstrap_key(storage_prefix: &str) -> String {
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
            if let Some(instance_id) = harness_instance_id() {
                let sanitized = sanitize_storage_segment(&instance_id);
                if !sanitized.is_empty() {
                    return format!("{WEB_STORAGE_PREFIX}{sanitized}_");
                }
            }
            WEB_STORAGE_PREFIX.to_string()
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

        pub(crate) fn load_selected_runtime_identity(
            storage_key: &str,
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
            Ok(None)
        }

        pub(crate) fn persist_selected_runtime_identity(
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

        pub(crate) fn load_pending_account_bootstrap(
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

        async fn persist_pending_web_account_bootstrap(
            nickname: &str,
        ) -> Result<PendingAccountBootstrap, WebUiError> {
            let pending_bootstrap = account_workflows::prepare_pending_account_bootstrap(nickname)
                .map_err(|error| {
                    WebUiError::input(
                        WebUiOperation::PersistPendingAccountBootstrap,
                        "WEB_PENDING_BOOTSTRAP_PREPARE_FAILED",
                        error.to_string(),
                    )
                })?;
            let storage_prefix = active_storage_prefix();
            let pending_account_key = pending_account_bootstrap_key(&storage_prefix);
            persist_pending_account_bootstrap(&pending_account_key, &pending_bootstrap)?;

            let staged_event = BootstrapEvent::new(
                BootstrapSurface::Web,
                BootstrapEventKind::PendingBootstrapStaged,
            );
            web_sys::console::log_1(&staged_event.to_string().into());
            Ok(pending_bootstrap)
        }

        pub(crate) async fn stage_runtime_bound_web_account_bootstrap(
            nickname: &str,
        ) -> Result<(), WebUiError> {
            let pending_bootstrap = persist_pending_web_account_bootstrap(nickname).await?;
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] staged_runtime_bound_account nickname={}",
                    pending_bootstrap.nickname_suggestion
                )
                .into(),
            );
            Ok(())
        }

        pub(crate) async fn stage_initial_web_account_bootstrap(
            nickname: &str,
        ) -> Result<(), WebUiError> {
            let pending_bootstrap = persist_pending_web_account_bootstrap(nickname).await?;
            let storage_prefix = active_storage_prefix();
            let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
            let random = RealRandomHandler::new();
            let authority_id = new_authority_id(&random).await;
            let device_id = new_device_id(&random).await;
            let runtime_identity = BootstrapRuntimeIdentity::new(authority_id, device_id);

            persist_selected_runtime_identity(&runtime_identity_key, &runtime_identity)?;
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] staged_initial_account authority={authority_id};device={device_id};nickname={}",
                    pending_bootstrap.nickname_suggestion
                )
                .into(),
            );
            Ok(())
        }

        pub(crate) fn device_enrollment_bootstrap_name(
            nickname_suggestion: Option<&str>,
        ) -> String {
            let nickname_suggestion = nickname_suggestion.unwrap_or("").trim();
            if nickname_suggestion.is_empty() {
                "Aura User".to_string()
            } else {
                nickname_suggestion.to_string()
            }
        }

        pub(crate) fn load_pending_device_enrollment_code(
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

        pub(crate) fn persist_pending_device_enrollment_code(
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

        pub(crate) fn clear_pending_device_enrollment_code(
            storage_key: &str,
        ) -> Result<(), WebUiError> {
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

        pub(crate) async fn submit_runtime_bootstrap_handoff(
            handoff: harness_bridge::BootstrapHandoff,
        ) -> Result<(), WebUiError> {
            harness_bridge::submit_bootstrap_handoff(handoff)
            .await
            .map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::SubmitBootstrapHandoff,
                    "WEB_BOOTSTRAP_HANDOFF_FAILED",
                    format!("failed to submit web bootstrap handoff: {:?}", error),
                )
            })
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
            let rebootstrap_lock = use_hook(|| Arc::new(Mutex::new(())));
            let shell_host = use_hook(move || {
                WebShellHost::new(
                    bootstrap_epoch,
                    committed_bootstrap,
                    bootstrap_error,
                    rebootstrap_lock.clone(),
                )
            });

            use_effect(|| {
                if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                    document.set_title("Aura");
                }
            });

            use_effect(move || {
                let submitter = shell_host.bootstrap_submitter();
                harness_bridge::set_bootstrap_handoff_submitter(submitter.clone());
                harness_bridge::set_runtime_identity_stager(
                    shell_host.runtime_identity_stager(submitter.clone()),
                );

                if !bootstrap_started.get() {
                    bootstrap_started.set(true);
                    let _ = submitter(harness_bridge::BootstrapHandoff::InitialBootstrap);
                }
            });

            if let Some(state) = committed_bootstrap() {
                return rsx! {
                    BootstrappedApp {
                        key: "{state.generation_id}",
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
            let account_ready = state.account_ready || controller_account_ready;

            if account_ready && !sync_loop_started() {
                sync_loop_started.set(true);
                spawn_background_sync_loop(controller.clone(), controller.app_core().clone());
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
                move |code: String| {
                    let mut import_error = import_error.clone();
                    let mut importing_code = importing_code.clone();
                    if importing_code() {
                        return;
                    }

                    let storage_prefix = active_storage_prefix();
                    importing_code.set(true);
                    import_error.set(None);

                    let controller = controller.clone();
                    let scheduled_controller = controller.clone();
                    if let Err(error) = harness_bridge::schedule_browser_task_next_tick(move || {
                        shared_web_task_owner().spawn_local(async move {
                            let app_core = scheduled_controller.app_core().clone();
                            let result: Result<_, WebUiError> = async {
                                let current_authority = runtime_workflows::require_runtime(&app_core)
                                    .await
                                    .map_err(|error| {
                                        WebUiError::operation(
                                            WebUiOperation::ImportDeviceEnrollmentCode,
                                            "WEB_RUNTIME_REQUIRED_FAILED",
                                            error.to_string(),
                                        )
                                    })?
                                    .authority_id();
                                let current_runtime_identity = CurrentRuntimeIdentity {
                                    authority_id: current_authority,
                                    selected_runtime_identity: logged_optional(
                                        load_selected_runtime_identity(&selected_runtime_identity_key(
                                            &storage_prefix,
                                        )),
                                    ),
                                };
                                let result = workflows::accept_device_enrollment_import(
                                    &app_core,
                                    DeviceEnrollmentImportRequest {
                                        code: &code,
                                        current_runtime_identity: current_runtime_identity.clone(),
                                        storage_prefix: &storage_prefix,
                                        rebootstrap_policy: RebootstrapPolicy::StageIfRequired,
                                        operation: WebUiOperation::ImportDeviceEnrollmentCode,
                                    },
                                )
                                .await?;
                                if result.rebootstrap_required {
                                    let staged_runtime_identity =
                                        result.staged_runtime_identity.clone();
                                    let selected_runtime_identity =
                                        current_runtime_identity.selected_runtime_identity;
                                    let device_id = staged_runtime_identity.device_id;
                                    let subject_authority = staged_runtime_identity.authority_id;
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
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] staged_rebootstrap subject_authority={};device_id={}",
                                        subject_authority, device_id
                                    )
                                    .into(),
                                );
                                submit_runtime_bootstrap_handoff(
                                    harness_bridge::BootstrapHandoff::RuntimeIdentityStaged {
                                        authority_id: subject_authority,
                                        device_id,
                                        source: harness_bridge::RuntimeIdentityStageSource::ImportDeviceEnrollment,
                                    },
                                )
                                .await
                                .map_err(|error| {
                                    error.with_operation(
                                        WebUiOperation::ImportDeviceEnrollmentCode,
                                    )
                                })?;
                                    return Ok(result);
                                }
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] accepting_on_bound_runtime authority={};selected_runtime_identity={:?};invited_device={}",
                                        current_authority,
                                        current_runtime_identity.selected_runtime_identity,
                                        result.staged_runtime_identity.device_id
                                    )
                                    .into(),
                                );
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] initializing_runtime_account nickname={}",
                                        result.bootstrap_name
                                    )
                                    .into(),
                                );
                                Ok(result)
                            }
                            .await;

                            match result {
                                Ok(result) => {
                                    if result.accepted {
                                        web_sys::console::log_1(
                                            &"[web-import-device] finalizing_ui".into(),
                                        );
                                        scheduled_controller.info_toast("Device enrollment complete");
                                        scheduled_controller
                                            .finalize_account_setup(ScreenId::Neighborhood);
                                        harness_bridge::publish_semantic_controller_snapshot(
                                            scheduled_controller.clone(),
                                        );
                                        web_sys::console::log_1(
                                            &"[web-import-device] finalized_ui".into(),
                                        );
                                    } else {
                                        scheduled_controller
                                            .info_toast("Switching runtime to finish import");
                                    }
                                    importing_code.set(false);
                                }
                                Err(error) => {
                                    let message = error.user_message();
                                    scheduled_controller.set_account_setup_state(
                                        false,
                                        "",
                                        Some(message.clone()),
                                    );
                                    import_error.set(Some(error));
                                    importing_code.set(false);
                                }
                            }
                        });
                    }) {
                        let error = WebUiError::operation(
                            WebUiOperation::ImportDeviceEnrollmentCode,
                            "WEB_DEVICE_ENROLLMENT_SCHEDULE_FAILED",
                            format!("{error:?}"),
                        );
                        controller.set_account_setup_state(
                            false,
                            "",
                            Some(error.user_message()),
                        );
                        import_error.set(Some(error));
                        importing_code.set(false);
                    }
                }
            });

            let submit_account = {
                let controller = controller.clone();
                let account_name = account_name.clone();
                let mut account_error = account_error.clone();
                let mut creating_account = creating_account.clone();
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
                        let result: Result<_, WebUiError> = async {
                            let result =
                                workflows::stage_account_creation(controller.app_core(), &nickname)
                                    .await?;
                            if result.mode == AccountCreationStageMode::InitialBootstrapStaged {
                                submit_runtime_bootstrap_handoff(
                                    harness_bridge::BootstrapHandoff::PendingAccountBootstrap {
                                        account_name: nickname.clone(),
                                        source: harness_bridge::PendingAccountBootstrapSource::OnboardingUi,
                                    },
                                )
                                .await?;
                            }
                            Ok(result)
                        }
                        .await;

                        match result {
                            Ok(result) => {
                                web_sys::console::log_1(
                                    &"[web-onboarding] submit_account ok".into(),
                                );
                                if result.mode == AccountCreationStageMode::RuntimeInitialized {
                                    controller
                                        .finalize_account_setup(ScreenId::Neighborhood);
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

            if !auto_import_started() {
                if let Some(pending_code) = state.pending_device_enrollment_code.clone() {
                    if !pending_code.is_empty() {
                        auto_import_started.set(true);
                        import_code.set(pending_code.clone());
                        let run_import = run_import.clone();
                        if let Err(error) =
                            harness_bridge::schedule_browser_task_next_tick(move || {
                                run_import(pending_code);
                            })
                        {
                            log_web_error(
                                "error",
                                &WebUiError::operation(
                                    WebUiOperation::ImportDeviceEnrollmentCode,
                                    "WEB_DEVICE_ENROLLMENT_AUTOSTART_SCHEDULE_FAILED",
                                    format!("{error:?}"),
                                ),
                            );
                        }
                    }
                }
            }

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

        pub(crate) fn spawn_browser_maintenance_loop<F, Fut>(
            controller: Arc<UiController>,
            app_core: Arc<RwLock<AppCore>>,
            interval_ms: u64,
            pause_message: &'static str,
            sleep_operation: WebUiOperation,
            sleep_error_code: &'static str,
            mut tick: F,
        ) where
            F: FnMut() -> Fut + 'static,
            Fut: Future<Output = ()> + 'static,
        {
            shared_web_task_owner().spawn_local_cancellable(async move {
                loop {
                    if let Err(error) = time_workflows::sleep_ms(&app_core, interval_ms).await {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                sleep_operation,
                                sleep_error_code,
                                error.to_string(),
                            ),
                        );
                        controller.runtime_error_toast(pause_message);
                        break;
                    }
                    tick().await;
                }
            });
        }

        fn spawn_background_sync_loop(
            controller: Arc<UiController>,
            app_core: Arc<RwLock<AppCore>>,
        ) {
            let tick_app_core = app_core.clone();
            spawn_browser_maintenance_loop(
                controller,
                app_core,
                1_500,
                "Background sync paused; refresh to resume",
                WebUiOperation::BackgroundSync,
                "WEB_BACKGROUND_SYNC_SLEEP_FAILED",
                move || {
                    let tick_app_core = tick_app_core.clone();
                    async move {
                        let runtime = { tick_app_core.read().await.runtime().cloned() };
                        if let Some(runtime) = runtime {
                            if let Err(error) = runtime_workflows::timeout_runtime_call(
                                &runtime,
                                "web_background_sync",
                                "trigger_discovery",
                                std::time::Duration::from_secs(3),
                                || runtime.trigger_discovery(),
                            )
                            .await
                            {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_DISCOVERY_TRIGGER_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                            if let Err(error) = runtime_workflows::timeout_runtime_call(
                                &runtime,
                                "web_background_sync",
                                "process_ceremony_messages_before_sync",
                                std::time::Duration::from_secs(3),
                                || runtime.process_ceremony_messages(),
                            )
                            .await
                            {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_CEREMONY_MESSAGES_BEFORE_SYNC_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                            if let Err(error) = runtime_workflows::timeout_runtime_call(
                                &runtime,
                                "web_background_sync",
                                "trigger_sync",
                                std::time::Duration::from_secs(3),
                                || runtime.trigger_sync(),
                            )
                            .await
                            {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_SYNC_TRIGGER_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                            if let Err(error) = runtime_workflows::timeout_runtime_call(
                                &runtime,
                                "web_background_sync",
                                "process_ceremony_messages_after_sync",
                                std::time::Duration::from_secs(3),
                                || runtime.process_ceremony_messages(),
                            )
                            .await
                            {
                                log_web_error(
                                    "warn",
                                    &WebUiError::operation(
                                        WebUiOperation::BackgroundSync,
                                        "WEB_CEREMONY_MESSAGES_AFTER_SYNC_FAILED",
                                        error.to_string(),
                                    ),
                                );
                            }
                        }
                        if let Err(error) = system_workflows::refresh_account(&tick_app_core).await
                        {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::BackgroundSync,
                                    "WEB_REFRESH_ACCOUNT_FAILED",
                                    error.to_string(),
                                ),
                            );
                        }
                        if let Err(error) =
                            network_workflows::refresh_discovered_peers(&tick_app_core).await
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
                    }
                },
            );
        }

    } else {
        fn main() {
            eprintln!("aura-web is a wasm32 frontend. Build with target wasm32-unknown-unknown.");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn web_harness_ui_state_observation_fails_closed_without_published_snapshot() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let source = std::fs::read_to_string(&bridge_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));

        assert!(source.contains("__AURA_UI_PUBLICATION_STATE__"));
        assert!(source.contains("semantic_snapshot_not_published"));
        assert!(!source.contains("return live_json;"));
    }

    #[test]
    fn web_harness_publication_failures_are_structurally_observable() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let source = std::fs::read_to_string(&bridge_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));

        assert!(source.contains("__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__"));
        assert!(source.contains("\"degraded\""));
        assert!(source.contains("\"unavailable\""));
        assert!(source.contains("driver_push_failed"));
    }

    #[test]
    fn web_background_sync_exit_is_structurally_visible() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let main_path = repo_root.join("crates/aura-web/src/main.rs");
        let source = std::fs::read_to_string(&main_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_path.display()));

        assert!(source.contains("fn spawn_browser_maintenance_loop<"));
        assert!(source.contains("controller.runtime_error_toast(pause_message);"));

        let helper_start = source
            .find("fn spawn_background_sync_loop")
            .unwrap_or_else(|| panic!("missing spawn_background_sync_loop"));
        let helper_end = source[helper_start..]
            .find("fn spawn_ceremony_acceptance_loop")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing spawn_ceremony_acceptance_loop"));
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("spawn_browser_maintenance_loop("));
        assert!(helper.contains("\"Background sync paused; refresh to resume\""));
        assert!(helper.contains("\"WEB_BACKGROUND_SYNC_SLEEP_FAILED\""));
    }

    #[test]
    fn web_ceremony_acceptance_exit_is_structurally_visible() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let helper_start = source
            .find("fn spawn_ceremony_acceptance_loop")
            .unwrap_or_else(|| panic!("missing spawn_ceremony_acceptance_loop"));
        let helper = &source[helper_start..];

        assert!(helper.contains("spawn_browser_maintenance_loop("));
        assert!(helper.contains("\"Ceremony acceptance paused; refresh to resume\""));
        assert!(helper.contains("\"WEB_CEREMONY_ACCEPTANCE_SLEEP_FAILED\""));
    }

    #[test]
    fn web_semantic_snapshot_publication_is_centralized() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let main_path = repo_root.join("crates/aura-web/src/main.rs");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let main_source = std::fs::read_to_string(&main_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_path.display()));
        let shell_host_source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });
        let bridge_source = std::fs::read_to_string(&bridge_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));
        let production_main = main_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_else(|| panic!("missing production main section"));

        assert!(bridge_source.contains("pub(crate) fn publish_semantic_controller_snapshot"));
        assert!(bridge_source.contains("controller.publish_ui_snapshot(snapshot.clone())"));
        assert!(shell_host_source
            .contains("harness_bridge::publish_semantic_controller_snapshot(controller.clone())"));
        assert!(!production_main
            .contains("harness_bridge::publish_semantic_controller_snapshot(controller.clone())"));
        assert!(!production_main.contains("harness_bridge::publish_ui_snapshot(&final_snapshot)"));
        assert!(!production_main.contains("harness_bridge::publish_ui_snapshot(&initial_snapshot)"));
    }

    #[test]
    fn web_bootstrap_sets_account_gate_before_initial_harness_publication() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let runtime_branch_start = source
            .find("let controller = Arc::new(UiController::with_authority_switcher(")
            .unwrap_or_else(|| panic!("missing runtime bootstrap controller"));
        let runtime_branch_end = source[runtime_branch_start..]
            .find("if account_ready {")
            .map(|offset| runtime_branch_start + offset)
            .unwrap_or_else(|| panic!("missing runtime bootstrap refresh branch"));
        let runtime_branch = &source[runtime_branch_start..runtime_branch_end];
        let runtime_gate_index = runtime_branch
            .find("controller.set_account_setup_state(account_ready, \"\", None);")
            .unwrap_or_else(|| panic!("missing runtime bootstrap account gate"));
        let runtime_install_index = runtime_branch
            .find("install_harness_instrumentation(controller.clone(), generation_id);")
            .unwrap_or_else(|| panic!("missing runtime bootstrap harness install"));
        assert!(
            runtime_gate_index < runtime_install_index,
            "runtime bootstrap must set the account gate before initial harness publication"
        );

        let shell_branch_start = source
            .find("let controller = Arc::new(UiController::new(app_core, clipboard));")
            .unwrap_or_else(|| panic!("missing shell bootstrap controller"));
        let shell_branch_end = source[shell_branch_start..]
            .find("let waiting_event = BootstrapEvent::new(")
            .map(|offset| shell_branch_start + offset)
            .unwrap_or_else(|| panic!("missing shell waiting event"));
        let shell_branch = &source[shell_branch_start..shell_branch_end];
        let shell_gate_index = shell_branch
            .find("controller.set_account_setup_state(false, \"\", None);")
            .unwrap_or_else(|| panic!("missing shell bootstrap account gate"));
        let shell_install_index = shell_branch
            .find("install_harness_instrumentation(controller.clone(), generation_id);")
            .unwrap_or_else(|| panic!("missing shell bootstrap harness install"));
        assert!(
            shell_gate_index < shell_install_index,
            "shell bootstrap must publish onboarding state only after applying the account gate"
        );
    }

    #[test]
    fn web_harness_selection_helpers_use_canonical_snapshot_selections_only() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let source = std::fs::read_to_string(&bridge_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));

        let channel_start = source
            .find(
                "fn selected_channel_id(controller: &UiController) -> Result<ChannelId, JsValue> {",
            )
            .unwrap_or_else(|| panic!("missing selected_channel_id"));
        let channel_end = source[channel_start..]
            .find("async fn selected_channel_binding(controller: &UiController)")
            .map(|offset| channel_start + offset)
            .unwrap_or(source.len());
        let channel_block = &source[channel_start..channel_end];
        assert!(channel_block.contains(".selected_channel_id()"));
        assert!(!channel_block.contains(".selected_item_id(ListId::Channels)"));

        let device_start = source
            .find("fn selected_device_id(controller: &UiController) -> Result<String, JsValue> {")
            .unwrap_or_else(|| panic!("missing selected_device_id"));
        let device_end = source[device_start..]
            .find("fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .map(|offset| device_start + offset)
            .unwrap_or(source.len());
        let device_block = &source[device_start..device_end];
        assert!(device_block.contains(".selected_item_id(ListId::Devices)"));
        assert!(!device_block.contains("list.items.len() == 1"));

        let authority_start = source
            .find("fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .unwrap_or_else(|| panic!("missing selected_authority_id"));
        let authority_end = source[authority_start..]
            .find("pub(crate) fn publish_semantic_controller_snapshot(controller: Arc<UiController>) -> UiSnapshot {")
            .map(|offset| authority_start + offset)
            .unwrap_or(source.len());
        let authority_block = &source[authority_start..authority_end];
        assert!(authority_block.contains(".selected_authority_id()"));
        assert!(!authority_block.contains(".selected_item_id(ListId::Authorities)"));
    }

    #[test]
    fn web_bootstrap_handoff_waits_for_completion() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let helper_start = source
            .find("pub(crate) async fn complete_handoff")
            .unwrap_or_else(|| panic!("missing complete_handoff"));
        let helper_end = source[helper_start..]
            .find("fn install_harness_instrumentation")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing install_harness_instrumentation"));
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("bootstrap_generation(epoch).await"));
        assert!(helper.contains("committed_bootstrap.set(None);"));
        assert!(helper.contains("harness_bridge::clear_controller("));
        assert!(helper.contains("harness_bridge::wait_for_generation_ready(generation_id)"));
        assert!(helper.contains("set_browser_shell_phase(BrowserShellPhase::Ready)"));
        assert!(helper.contains("Err(error)"));
        assert!(!helper.contains("spawn_local(async move"));
    }

    #[test]
    fn web_runtime_account_paths_persist_browser_account_config() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let main_path = repo_root.join("crates/aura-web/src/main.rs");
        let source = std::fs::read_to_string(&main_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_path.display()));

        assert!(
            source.contains("workflows::accept_device_enrollment_import("),
            "device enrollment import should route through the shared aura-web workflow helper"
        );
        assert!(
            source.contains("workflows::stage_account_creation(controller.app_core(), &nickname)"),
            "onboarding account creation should route through the shared aura-web workflow helper"
        );
    }
}

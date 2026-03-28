use async_lock::{Mutex, RwLock};
use aura_agent::AgentBuilder;
use aura_app::ui::types::{
    BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity, BootstrapSurface,
};
use aura_app::ui::workflows::account as account_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::{AppConfig, AppCore};
use aura_core::types::identifiers::AuthorityId;
use aura_ui::{FrontendUiOperation as WebUiOperation, UiController};
use dioxus::prelude::{Signal, WritableExt};
use std::sync::Arc;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::future_to_promise;

use crate::bootstrap_storage::{load_persisted_account_config, persist_runtime_account_config};
use crate::error::{log_web_error, WebUiError};
use crate::harness::generation::BrowserShellPhase;
use crate::harness_bridge::{self, BootstrapHandoff, RuntimeIdentityStageSource};
use crate::shell::{cancel_generation_maintenance_loops, spawn_generation_maintenance_loops};
use crate::task_owner::shared_web_task_owner;
use crate::web_clipboard::WebClipboardAdapter;
use crate::{
    active_storage_prefix, clear_storage_key, harness_instance_id, harness_mode_enabled,
    load_pending_account_bootstrap, load_pending_device_enrollment_code,
    load_selected_runtime_identity, logged_optional, pending_account_bootstrap_key,
    pending_device_enrollment_code_key, persist_selected_runtime_identity,
    selected_runtime_identity_key, submit_runtime_bootstrap_handoff,
    workflows::{self, CurrentRuntimeIdentity, DeviceEnrollmentImportRequest, RebootstrapPolicy},
};

#[derive(Clone, PartialEq)]
pub(crate) struct BootstrapState {
    pub(crate) generation_id: u64,
    pub(crate) controller: Arc<UiController>,
    pub(crate) account_ready: bool,
    pub(crate) pending_device_enrollment_code: Option<String>,
    pub(crate) pending_device_enrollment_code_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BootstrapPhase {
    LoadStorageKeys,
    ResolveRuntimeIdentity,
    BuildAgent,
    InitAppCore,
    HydrateExistingAccount,
    ReconcilePendingBootstrap,
    ReconcilePendingEnrollment,
    InstallHarness,
    SpawnMaintenance,
    PublishFinalSnapshot,
    Ready,
}

impl BootstrapPhase {
    const fn order(self) -> u8 {
        match self {
            Self::LoadStorageKeys => 0,
            Self::ResolveRuntimeIdentity => 1,
            Self::BuildAgent => 2,
            Self::InitAppCore => 3,
            Self::HydrateExistingAccount => 4,
            Self::ReconcilePendingBootstrap => 5,
            Self::ReconcilePendingEnrollment => 6,
            Self::InstallHarness => 7,
            Self::SpawnMaintenance => 8,
            Self::PublishFinalSnapshot => 9,
            Self::Ready => 10,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::LoadStorageKeys => "load_storage_keys",
            Self::ResolveRuntimeIdentity => "resolve_runtime_identity",
            Self::BuildAgent => "build_agent",
            Self::InitAppCore => "init_app_core",
            Self::HydrateExistingAccount => "hydrate_existing_account",
            Self::ReconcilePendingBootstrap => "reconcile_pending_bootstrap",
            Self::ReconcilePendingEnrollment => "reconcile_pending_enrollment",
            Self::InstallHarness => "install_harness",
            Self::SpawnMaintenance => "spawn_maintenance",
            Self::PublishFinalSnapshot => "publish_final_snapshot",
            Self::Ready => "ready",
        }
    }
}

#[derive(Clone, Debug)]
struct BootstrapPhaseTracker {
    generation_id: u64,
    current: BootstrapPhase,
}

impl BootstrapPhaseTracker {
    fn new(generation_id: u64) -> Self {
        Self {
            generation_id,
            current: BootstrapPhase::LoadStorageKeys,
        }
    }

    fn advance_to(&mut self, next: BootstrapPhase) -> Result<(), WebUiError> {
        if next.order() <= self.current.order() {
            return Err(self.error(
                WebUiOperation::BootstrapController,
                "WEB_BOOTSTRAP_PHASE_ORDER_INVALID",
                format!(
                    "invalid bootstrap phase transition {} -> {} for generation {}",
                    self.current.label(),
                    next.label(),
                    self.generation_id
                ),
            ));
        }
        self.current = next;
        Ok(())
    }

    fn error(
        &self,
        operation: WebUiOperation,
        code: &'static str,
        detail: impl Into<String>,
    ) -> WebUiError {
        WebUiError::operation(
            operation,
            code,
            format!(
                "bootstrap phase {} (generation {}): {}",
                self.current.label(),
                self.generation_id,
                detail.into()
            ),
        )
    }
}

#[derive(Clone)]
pub(crate) struct WebShellHost {
    rebootstrap_lock: Arc<Mutex<()>>,
    bootstrap_epoch: Signal<u64>,
    committed_bootstrap: Signal<Option<BootstrapState>>,
    bootstrap_error: Signal<Option<WebUiError>>,
}

impl WebShellHost {
    pub(crate) fn new(
        bootstrap_epoch: Signal<u64>,
        committed_bootstrap: Signal<Option<BootstrapState>>,
        bootstrap_error: Signal<Option<WebUiError>>,
        rebootstrap_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            rebootstrap_lock,
            bootstrap_epoch,
            committed_bootstrap,
            bootstrap_error,
        }
    }

    pub(crate) fn bootstrap_submitter(&self) -> Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise> {
        let host = self.clone();
        Arc::new(move |handoff| {
            let host = host.clone();
            future_to_promise(async move {
                host.complete_handoff(handoff)
                    .await
                    .map_err(|error| JsValue::from_str(&error.user_message()))?;
                Ok(JsValue::UNDEFINED)
            })
        })
    }

    pub(crate) fn runtime_identity_stager(
        &self,
        submitter: Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>,
    ) -> Arc<dyn Fn(String) -> js_sys::Promise> {
        Arc::new(move |serialized_identity| {
            let submitter = submitter.clone();
            future_to_promise(async move {
                let runtime_identity: BootstrapRuntimeIdentity =
                    serde_json::from_str(&serialized_identity).map_err(|error| {
                        JsValue::from_str(&format!(
                            "failed to parse staged runtime identity: {error}"
                        ))
                    })?;
                let storage_prefix = active_storage_prefix();
                let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
                persist_selected_runtime_identity(&runtime_identity_key, &runtime_identity)
                    .map_err(|error| JsValue::from_str(&error.user_message()))?;
                let handoff = BootstrapHandoff::RuntimeIdentityStaged {
                    authority_id: runtime_identity.authority_id,
                    device_id: runtime_identity.device_id,
                    source: RuntimeIdentityStageSource::HarnessStaging,
                };
                let _ = wasm_bindgen_futures::JsFuture::from(submitter(handoff)).await?;
                Ok(JsValue::UNDEFINED)
            })
        })
    }

    pub(crate) async fn complete_handoff(
        &self,
        handoff: BootstrapHandoff,
    ) -> Result<(), WebUiError> {
        let _guard = self.rebootstrap_lock.lock().await;
        let mut bootstrap_epoch = self.bootstrap_epoch;
        let mut committed_bootstrap = self.committed_bootstrap;
        let mut bootstrap_error = self.bootstrap_error;

        let epoch = bootstrap_epoch() + 1;
        web_sys::console::log_1(
            &format!(
                "[web-bootstrap] handoff start epoch={epoch} detail={}",
                handoff.detail()
            )
            .into(),
        );
        harness_bridge::set_browser_shell_phase(BrowserShellPhase::Bootstrapping);
        if !matches!(handoff, BootstrapHandoff::InitialBootstrap) {
            harness_bridge::set_browser_shell_phase(BrowserShellPhase::HandoffCommitted);
        }
        bootstrap_epoch.set(epoch);
        bootstrap_error.set(None);
        committed_bootstrap.set(None);
        cancel_generation_maintenance_loops();
        harness_bridge::clear_controller(&format!("bootstrap_generation_rebinding:{epoch}"));
        harness_bridge::set_browser_shell_phase(BrowserShellPhase::Rebinding);

        web_sys::console::log_1(
            &format!(
                "[web-bootstrap] runner start epoch={epoch} detail={}",
                handoff.detail()
            )
            .into(),
        );

        match bootstrap_generation(epoch).await {
            Ok(state) => {
                web_sys::console::log_1(
                    &format!(
                        "[web-bootstrap] runner ok epoch={epoch} generation={} detail={}",
                        state.generation_id,
                        handoff.detail(),
                    )
                    .into(),
                );
                bootstrap_error.set(None);
                let generation_id = state.generation_id;
                let account_ready = state.account_ready;
                committed_bootstrap.set(Some(state));
                if account_ready {
                    harness_bridge::wait_for_generation_ready(generation_id)
                        .await
                        .map_err(|error| {
                            WebUiError::operation(
                                WebUiOperation::SubmitBootstrapHandoff,
                                "WEB_BOOTSTRAP_GENERATION_READY_WAIT_FAILED",
                                format!(
                                    "failed waiting for generation {generation_id} publication readiness: {error:?}"
                                ),
                            )
                        })?;
                }
                harness_bridge::set_browser_shell_phase(BrowserShellPhase::Ready);
                Ok(())
            }
            Err(error) => {
                web_sys::console::error_1(
                    &format!(
                        "[web-bootstrap] runner error epoch={epoch} detail={} error={}",
                        handoff.detail(),
                        error.user_message()
                    )
                    .into(),
                );
                if committed_bootstrap().is_none() {
                    bootstrap_error.set(Some(error.clone()));
                } else {
                    log_web_error("error", &error);
                }
                Err(error)
            }
        }
    }
}

async fn hydrate_existing_runtime_account_projection(
    app_core: &Arc<RwLock<AppCore>>,
    persisted_account_config: Option<&aura_app::ui::types::AccountConfig>,
) -> Result<(), WebUiError> {
    let account_ready = account_workflows::has_runtime_bootstrapped_account(app_core)
        .await
        .map_err(|error| {
            WebUiError::operation(
                WebUiOperation::BootstrapController,
                "WEB_BOOTSTRAP_ACCOUNT_READY_CHECK_FAILED",
                format!("failed to inspect runtime account readiness before hydration: {error}"),
            )
        })?;
    if account_ready {
        return Ok(());
    }
    if let Some(account_config) = persisted_account_config {
        if let Some(nickname_suggestion) = account_config.nickname_suggestion.clone() {
            account_workflows::initialize_runtime_account(app_core, nickname_suggestion)
                .await
                .map_err(|error| {
                    WebUiError::operation(
                        WebUiOperation::BootstrapController,
                        "WEB_BOOTSTRAP_ACCOUNT_INIT_FROM_PERSISTED_CONFIG_FAILED",
                        format!(
                            "failed to initialize runtime account from persisted account config: {error}"
                        ),
                    )
                })?;
            return Ok(());
        }
        log_web_error(
            "warn",
            &WebUiError::operation(
                WebUiOperation::BootstrapController,
                "WEB_BOOTSTRAP_ACCOUNT_CONFIG_NICKNAME_MISSING",
                format!(
                    "persisted browser account config for authority {} is missing a nickname suggestion",
                    account_config.authority_id
                ),
            ),
        );
    }
    if let Err(error) = system_workflows::refresh_account(app_core).await {
        log_web_error(
            "warn",
            &WebUiError::operation(
                WebUiOperation::BootstrapController,
                "WEB_BOOTSTRAP_REFRESH_ACCOUNT_FAILED",
                format!("failed to hydrate runtime account projection during bootstrap: {error}"),
            ),
        );
    }
    Ok(())
}

async fn reconcile_pending_device_enrollment_import(
    app_core: &Arc<RwLock<AppCore>>,
    runtime_authority_id: AuthorityId,
    runtime_device_id: aura_core::types::identifiers::DeviceId,
    pending_code: &str,
    pending_code_storage_key: &str,
) -> Result<Option<String>, WebUiError> {
    let storage_prefix = active_storage_prefix();
    let result = workflows::accept_device_enrollment_import(
        app_core,
        DeviceEnrollmentImportRequest {
            code: pending_code,
            current_runtime_identity: CurrentRuntimeIdentity {
                authority_id: runtime_authority_id,
                selected_runtime_identity: Some(BootstrapRuntimeIdentity::new(
                    runtime_authority_id,
                    runtime_device_id,
                )),
            },
            storage_prefix: &storage_prefix,
            rebootstrap_policy: RebootstrapPolicy::RejectIfRequired,
            operation: WebUiOperation::BootstrapController,
        },
    )
    .await?;
    if result.rebootstrap_required {
        let staged_runtime_identity = result.staged_runtime_identity;
        return Err(WebUiError::operation(
            WebUiOperation::BootstrapController,
            "WEB_PENDING_DEVICE_ENROLLMENT_RUNTIME_IDENTITY_MISMATCH",
            format!(
                "pending device enrollment code expected authority {subject_authority} device {device_id}, but bootstrap runtime is authority {} device {}",
                runtime_authority_id, runtime_device_id,
                subject_authority = staged_runtime_identity.authority_id,
                device_id = staged_runtime_identity.device_id,
            ),
        ));
    }
    let _ = pending_code_storage_key;
    Ok(Some(result.bootstrap_name))
}

fn install_harness_instrumentation(
    controller: Arc<UiController>,
    generation_id: u64,
) {
    if !harness_mode_enabled() {
        return;
    }
    controller.set_ui_snapshot_sink(Arc::new(|snapshot| {
        harness_bridge::publish_ui_snapshot(&snapshot);
    }));

    harness_bridge::set_active_generation(generation_id);
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
    let _ = harness_bridge::publish_semantic_controller_snapshot(controller);
}

async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {
    let mut phase = BootstrapPhaseTracker::new(generation_id);
    let storage_prefix = active_storage_prefix();
    let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
    let pending_account_key = pending_account_bootstrap_key(&storage_prefix);
    let pending_code_storage_key = pending_device_enrollment_code_key(&storage_prefix);
    let selected_runtime_identity =
        logged_optional(load_selected_runtime_identity(&runtime_identity_key));
    let pending_account_bootstrap =
        logged_optional(load_pending_account_bootstrap(&pending_account_key));
    let pending_device_enrollment_code = logged_optional(load_pending_device_enrollment_code(
        &pending_code_storage_key,
    ));
    web_sys::console::log_1(
        &format!(
            "[web-bootstrap] generation={generation_id};storage_prefix={storage_prefix};selected_runtime_identity={:?};pending_account_bootstrap={:?};pending_device_enrollment_code_present={}",
            selected_runtime_identity,
            pending_account_bootstrap,
            pending_device_enrollment_code
                .as_ref()
                .is_some_and(|code| !code.is_empty())
        )
        .into(),
    );
    phase.advance_to(BootstrapPhase::ResolveRuntimeIdentity)?;
    let harness_instance = harness_instance_id();
    let clipboard = Arc::new(WebClipboardAdapter::default());
    if let Some(runtime_identity) = selected_runtime_identity {
        let persisted_account_config =
            logged_optional(load_persisted_account_config(WebUiOperation::BootstrapController))
                .and_then(|account_config| {
                    if account_config.authority_id == runtime_identity.authority_id {
                        Some(account_config)
                    } else {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::BootstrapController,
                                "WEB_BOOTSTRAP_ACCOUNT_CONFIG_AUTHORITY_MISMATCH",
                                format!(
                                    "persisted browser account config authority {} does not match selected runtime authority {}",
                                    account_config.authority_id,
                                    runtime_identity.authority_id
                                ),
                            ),
                        );
                        None
                    }
                });
        let authority_id = runtime_identity.authority_id;
        let device_id = runtime_identity.device_id;
        let config = aura_agent::core::AgentConfig {
            device_id,
            ..Default::default()
        };
        phase.advance_to(BootstrapPhase::BuildAgent)?;
        let agent = Arc::new(
            AgentBuilder::web()
                .storage_prefix(&storage_prefix)
                .authority(authority_id)
                .with_config(config)
                .build()
                .await
                .map_err(|error| {
                    phase.error(
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

        phase.advance_to(BootstrapPhase::InitAppCore)?;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), agent.clone().as_runtime_bridge())
                .map_err(|error| {
                    phase.error(
                        WebUiOperation::BootstrapController,
                        "WEB_APP_CORE_INIT_FAILED",
                        format!("failed to initialize AppCore: {error}"),
                    )
                })?,
        ));

        AppCore::init_signals_with_hooks(&app_core)
            .await
            .map_err(|error| {
                phase.error(
                    WebUiOperation::BootstrapController,
                    "WEB_SIGNAL_INIT_FAILED",
                    format!("failed to initialize app signals: {error}"),
                )
            })?;

        if pending_account_bootstrap.is_none() && pending_device_enrollment_code.is_none() {
            phase.advance_to(BootstrapPhase::HydrateExistingAccount)?;
            hydrate_existing_runtime_account_projection(
                &app_core,
                persisted_account_config.as_ref(),
            )
            .await
            .map_err(|error| {
                phase.error(
                    WebUiOperation::BootstrapController,
                    "WEB_BOOTSTRAP_HYDRATE_EXISTING_ACCOUNT_FAILED",
                    error.user_message(),
                )
            })?;
        }

        phase.advance_to(BootstrapPhase::ReconcilePendingBootstrap)?;
        let bootstrap_resolution = account_workflows::reconcile_pending_runtime_account_bootstrap(
            &app_core,
            pending_account_bootstrap.clone(),
        )
        .await
        .map_err(|error| {
            phase.error(
                WebUiOperation::BootstrapController,
                "WEB_PENDING_BOOTSTRAP_RECONCILE_FAILED",
                format!("failed to reconcile pending web account bootstrap: {error}"),
            )
        })?;
        let mut account_ready = bootstrap_resolution.account_ready;

        if bootstrap_resolution.action != account_workflows::PendingRuntimeBootstrapAction::None {
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
        if let Err(error) =
            persist_selected_runtime_identity(&runtime_identity_key, &current_runtime_identity)
        {
            log_web_error("warn", &error);
        }

        let current_device_id = current_runtime_identity.device_id;
        let controller = Arc::new(UiController::with_authority_switcher(
            app_core.clone(),
            clipboard,
            Some(Arc::new(move |authority_id: AuthorityId| {
                let storage_prefix = active_storage_prefix();
                let runtime_identity_key = selected_runtime_identity_key(&storage_prefix);
                let runtime_identity = load_selected_runtime_identity(&runtime_identity_key);
                let runtime_identity = logged_optional(runtime_identity).unwrap_or_else(|| {
                    BootstrapRuntimeIdentity::new(authority_id, current_device_id.clone())
                });
                let updated_identity =
                    BootstrapRuntimeIdentity::new(authority_id, runtime_identity.device_id);
                if let Err(error) =
                    persist_selected_runtime_identity(&runtime_identity_key, &updated_identity)
                {
                    log_web_error("error", &error);
                    return;
                }
                shared_web_task_owner().spawn_local(async move {
                    if let Err(error) =
                        submit_runtime_bootstrap_handoff(BootstrapHandoff::RuntimeIdentityStaged {
                            authority_id,
                            device_id: updated_identity.device_id,
                            source: RuntimeIdentityStageSource::AuthoritySwitch,
                        })
                        .await
                    {
                        log_web_error("error", &error);
                    }
                });
            })),
        ));
        phase.advance_to(BootstrapPhase::ReconcilePendingEnrollment)?;
        if let Some(pending_code) = pending_device_enrollment_code
            .as_ref()
            .filter(|code| !code.is_empty())
        {
            let pending_code = pending_code.clone();
            if reconcile_pending_device_enrollment_import(
                &app_core,
                authority_id,
                device_id,
                &pending_code,
                &pending_code_storage_key,
            )
            .await
            .map_err(|error| {
                phase.error(
                    WebUiOperation::BootstrapController,
                    "WEB_PENDING_DEVICE_ENROLLMENT_RECONCILE_FAILED",
                    error.user_message(),
                )
            })?
            .is_some()
            {
                account_ready = true;
            }
        }
        controller.set_account_setup_state(account_ready, "", None);
        if account_ready {
            controller.finalize_account_setup(aura_app::ui::contract::ScreenId::Neighborhood);
        }
        phase.advance_to(BootstrapPhase::InstallHarness)?;
        install_harness_instrumentation(controller.clone(), generation_id);
        if account_ready {
            if let Err(error) =
                settings_workflows::refresh_settings_from_runtime(controller.app_core()).await
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
            if let Err(error) = persist_runtime_account_config(
                &app_core,
                pending_account_bootstrap
                    .as_ref()
                    .map(|pending_bootstrap| pending_bootstrap.nickname_suggestion.clone())
                    .or_else(|| {
                        persisted_account_config
                            .as_ref()
                            .and_then(|account_config| account_config.nickname_suggestion.clone())
                    }),
                WebUiOperation::BootstrapController,
            )
            .await
            {
                log_web_error("warn", &error);
            }
            match runtime_workflows::require_runtime(controller.app_core()).await {
                Ok(runtime) => {
                    let runtime_devices = match runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "web_bootstrap",
                        "try_list_devices",
                        std::time::Duration::from_secs(3),
                        || runtime.try_list_devices(),
                    )
                    .await
                    {
                        Ok(Ok(devices)) => devices,
                        Ok(Err(error)) => {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::InspectBootstrapRuntime,
                                    "WEB_BOOTSTRAP_RUNTIME_LIST_DEVICES_FAILED",
                                    error.to_string(),
                                ),
                            );
                            Vec::new()
                        }
                        Err(error) => {
                            log_web_error(
                                "warn",
                                &WebUiError::operation(
                                    WebUiOperation::InspectBootstrapRuntime,
                                    "WEB_BOOTSTRAP_RUNTIME_LIST_DEVICES_TIMEOUT",
                                    error.to_string(),
                                ),
                            );
                            Vec::new()
                        }
                    };
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

        phase.advance_to(BootstrapPhase::SpawnMaintenance)?;
        spawn_generation_maintenance_loops(
            generation_id,
            controller.clone(),
            app_core.clone(),
            account_ready,
            Some(agent.clone()),
        );
        phase.advance_to(BootstrapPhase::PublishFinalSnapshot)?;
        let final_snapshot =
            harness_bridge::publish_semantic_controller_snapshot(controller.clone());
        web_sys::console::log_1(
            &format!(
                "[web-bootstrap] final_snapshot screen={:?};readiness={:?};revision={:?}",
                final_snapshot.screen, final_snapshot.readiness, final_snapshot.revision
            )
            .into(),
        );
        if account_ready {
            let finalized_event = BootstrapEvent::new(
                BootstrapSurface::Web,
                BootstrapEventKind::RuntimeBootstrapFinalized,
            );
            controller.push_log(&finalized_event.to_string());
        }
        if let Some(instance_id) = harness_instance {
            controller.push_log(&format!(
                "web harness instance {instance_id} booted in testing mode"
            ));
        }
        phase.advance_to(BootstrapPhase::Ready)?;
        Ok(BootstrapState {
            generation_id,
            controller,
            account_ready,
            pending_device_enrollment_code: pending_device_enrollment_code
                .filter(|code| !code.is_empty()),
            pending_device_enrollment_code_key: pending_code_storage_key,
        })
    } else {
        phase.advance_to(BootstrapPhase::InitAppCore)?;
        let app_core = Arc::new(RwLock::new(AppCore::new(AppConfig::default()).map_err(
            |error| {
                phase.error(
                    WebUiOperation::BootstrapController,
                    "WEB_BOOTSTRAP_APP_CORE_INIT_FAILED",
                    format!("failed to initialize bootstrap AppCore: {error}"),
                )
            },
        )?));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .map_err(|error| {
                phase.error(
                    WebUiOperation::BootstrapController,
                    "WEB_BOOTSTRAP_SIGNAL_INIT_FAILED",
                    format!("failed to initialize bootstrap app signals: {error}"),
                )
            })?;
        let controller = Arc::new(UiController::new(app_core, clipboard));
        controller.set_account_setup_state(false, "", None);
        phase.advance_to(BootstrapPhase::InstallHarness)?;
        install_harness_instrumentation(controller.clone(), generation_id);
        let waiting_event = BootstrapEvent::new(
            BootstrapSurface::Web,
            BootstrapEventKind::ShellAwaitingAccount,
        );
        controller.push_log(&waiting_event.to_string());
        phase.advance_to(BootstrapPhase::PublishFinalSnapshot)?;
        let _ = harness_bridge::publish_semantic_controller_snapshot(controller.clone());
        if let Some(instance_id) = harness_instance {
            controller.push_log(&format!(
                "web harness instance {instance_id} booted without runtime"
            ));
        }
        phase.advance_to(BootstrapPhase::Ready)?;
        Ok(BootstrapState {
            generation_id,
            controller,
            account_ready: false,
            pending_device_enrollment_code: None,
            pending_device_enrollment_code_key: pending_code_storage_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BootstrapPhase, BootstrapPhaseTracker};
    use aura_ui::FrontendUiOperation as WebUiOperation;

    #[test]
    fn bootstrap_phase_tracker_enforces_monotonic_ordering() {
        let mut tracker = BootstrapPhaseTracker::new(7);
        tracker
            .advance_to(BootstrapPhase::ResolveRuntimeIdentity)
            .expect("forward transition should succeed");
        tracker
            .advance_to(BootstrapPhase::InitAppCore)
            .expect("skipping forward within the declared order should succeed");
        let error = tracker
            .advance_to(BootstrapPhase::ResolveRuntimeIdentity)
            .expect_err("backward transition must fail");

        let rendered = error.user_message();
        assert!(rendered.contains("WEB_BOOTSTRAP_PHASE_ORDER_INVALID"));
        assert!(rendered.contains("init_app_core"));
        assert!(rendered.contains("resolve_runtime_identity"));
    }

    #[test]
    fn bootstrap_phase_tracker_errors_include_phase_context() {
        let tracker = BootstrapPhaseTracker::new(11);
        let error = tracker.error(
            WebUiOperation::BootstrapController,
            "WEB_BOOTSTRAP_PHASE_CONTEXT_TEST",
            "phase-aware detail",
        );
        let rendered = error.user_message();

        assert!(rendered.contains("WEB_BOOTSTRAP_PHASE_CONTEXT_TEST"));
        assert!(rendered.contains("load_storage_keys"));
        assert!(rendered.contains("generation 11"));
    }

    #[test]
    fn shell_host_hydrates_runtime_projection_before_pending_reconcile() {
        let source = include_str!("shell_host.rs");
        let bootstrap_start = source
            .find("async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {")
            .unwrap_or_else(|| panic!("missing bootstrap_generation"));
        let bootstrap_end = source[bootstrap_start..]
            .find("\n#[cfg(test)]")
            .map(|offset| bootstrap_start + offset)
            .unwrap_or(source.len());
        let bootstrap_block = &source[bootstrap_start..bootstrap_end];
        assert!(
            bootstrap_block.contains("if pending_account_bootstrap.is_none() && pending_device_enrollment_code.is_none() {\n            hydrate_existing_runtime_account_projection("),
            "runtime-backed web bootstrap should only fall back to persisted account hydration when there is no pending device-enrollment continuation to reconcile first"
        );
    }

    #[test]
    fn shell_host_reconciles_pending_device_enrollment_before_ready() {
        let source = include_str!("shell_host.rs");
        let bootstrap_start = source
            .find("async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {")
            .unwrap_or_else(|| panic!("missing bootstrap_generation"));
        let bootstrap_end = source[bootstrap_start..]
            .find("\n#[cfg(test)]")
            .map(|offset| bootstrap_start + offset)
            .unwrap_or(source.len());
        let bootstrap_block = &source[bootstrap_start..bootstrap_end];
        assert!(
            bootstrap_block.contains("if let Some(pending_code) = pending_device_enrollment_code"),
            "bootstrap_generation should reconcile pending device enrollment imports inside the shell host before declaring the new generation ready"
        );
        assert!(
            bootstrap_block.contains("reconcile_pending_device_enrollment_import("),
            "runtime-identity staged imports should continue through a shell-host owned reconciliation path"
        );
    }

    #[test]
    fn shell_host_only_waits_for_generation_ready_when_account_ready() {
        let source = include_str!("shell_host.rs");
        let handoff_start = source
            .find("pub(crate) async fn complete_handoff(")
            .unwrap_or_else(|| panic!("missing complete_handoff"));
        let handoff_end = source[handoff_start..]
            .find("async fn hydrate_existing_runtime_account_projection(")
            .map(|offset| handoff_start + offset)
            .unwrap_or(source.len());
        let handoff_block = &source[handoff_start..handoff_end];
        assert!(
            handoff_block.contains("if account_ready {\n                    harness_bridge::wait_for_generation_ready(generation_id)"),
            "bootstrap handoff completion should wait for generation-ready publication only when the promoted generation is semantically ready"
        );
    }

    #[test]
    fn shell_host_only_logs_runtime_bootstrap_finalized_for_ready_generations() {
        let source = include_str!("shell_host.rs");
        let bootstrap_start = source
            .find("async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {")
            .unwrap_or_else(|| panic!("missing bootstrap_generation"));
        let bootstrap_end = source[bootstrap_start..]
            .find("\n#[cfg(test)]")
            .map(|offset| bootstrap_start + offset)
            .unwrap_or(source.len());
        let bootstrap_block = &source[bootstrap_start..bootstrap_end];
        assert!(
            bootstrap_block.contains(
                "if account_ready {\n            let finalized_event = BootstrapEvent::new("
            ),
            "runtime bootstrap finalized logs must only be published for semantically ready generations"
        );
    }

    #[test]
    fn shell_host_persists_runtime_account_config_for_ready_generations() {
        let source = include_str!("shell_host.rs");
        let bootstrap_start = source
            .find("async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {")
            .unwrap_or_else(|| panic!("missing bootstrap_generation"));
        let bootstrap_end = source[bootstrap_start..]
            .find("\n#[cfg(test)]")
            .map(|offset| bootstrap_start + offset)
            .unwrap_or(source.len());
        let bootstrap_block = &source[bootstrap_start..bootstrap_end];
        assert!(
            bootstrap_block.contains("if let Err(error) = persist_runtime_account_config("),
            "ready browser generations must persist account bootstrap metadata for preserved-profile restarts"
        );
    }
}

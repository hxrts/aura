//! JavaScript harness API bridge for browser-based testing.
//!
//! Exposes the UiController to JavaScript via window.harness, enabling the test
//! harness to send keys, capture screenshots, and query UI state from Playwright.

use aura_app::ui::contract::{
    classify_screen_item_id, classify_semantic_settings_section_item_id, ListId, ModalId,
    OperationId, OperationInstanceId, RenderHeartbeat, ScreenId, UiReadiness, UiSnapshot,
};
use aura_app::ui::scenarios::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue,
    SemanticSubmissionHandle, SettingsSection, SubmissionState, UiOperationHandle,
};
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::types::{BootstrapRuntimeIdentity, InvitationBridgeType};
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::invitation as invitation_workflows;
use aura_app::ui::workflows::messaging as messaging_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings as settings_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::ui_contract::{
    RuntimeFact, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalStatus,
};
use aura_core::{types::identifiers::ChannelId, AuthorityId, DeviceId};
use aura_effects::ReactiveEffects;
use aura_ui::UiController;
use futures::channel::oneshot;
use js_sys::{Array, Function, Object, Reflect, JSON};
use serde_json::{from_str, to_string};
use serde_wasm_bindgen::to_value;
use std::cell::{Cell, RefCell, RefCell as StdRefCell};
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::future_to_promise;

use crate::{
    active_storage_prefix, load_selected_runtime_identity, selected_runtime_identity_key,
    submit_runtime_bootstrap_handoff,
    task_owner::shared_web_task_owner,
    workflows::{
        self, AccountCreationStageMode, CurrentRuntimeIdentity, DeviceEnrollmentImportRequest,
        RebootstrapPolicy,
    },
};

thread_local! {
    static CONTROLLER: RefCell<Option<Arc<UiController>>> = const { RefCell::new(None) };
    static LAST_PUBLISHED_UI_STATE_JSON: RefCell<Option<String>> = const { RefCell::new(None) };
    static RENDER_SEQ: RefCell<u64> = const { RefCell::new(0) };
    static ACTIVE_GENERATION: Cell<u64> = const { Cell::new(0) };
    static READY_GENERATION: Cell<u64> = const { Cell::new(0) };
    static BROWSER_SHELL_PHASE: Cell<BrowserShellPhase> = const { Cell::new(BrowserShellPhase::Bootstrapping) };
    static GENERATION_READY_WAITERS: RefCell<Vec<(u64, oneshot::Sender<()>)>> = const { RefCell::new(Vec::new()) };
    static BOOTSTRAP_HANDOFF_SUBMITTER: RefCell<Option<Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>>> = const { RefCell::new(None) };
    static RUNTIME_IDENTITY_STAGER: RefCell<Option<Arc<dyn Fn(String) -> js_sys::Promise>>> = const { RefCell::new(None) };
}

const UI_PUBLICATION_STATE_KEY: &str = "__AURA_UI_PUBLICATION_STATE__";
const RENDER_HEARTBEAT_PUBLICATION_STATE_KEY: &str = "__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__";
const SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY: &str = "__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__";
const UI_ACTIVE_GENERATION_KEY: &str = "__AURA_UI_ACTIVE_GENERATION__";
const UI_READY_GENERATION_KEY: &str = "__AURA_UI_READY_GENERATION__";
const UI_GENERATION_PHASE_KEY: &str = "__AURA_UI_GENERATION_PHASE__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserShellPhase {
    Bootstrapping,
    HandoffCommitted,
    Rebinding,
    Ready,
}

fn browser_shell_phase_label(phase: BrowserShellPhase) -> &'static str {
    match phase {
        BrowserShellPhase::Bootstrapping => "bootstrapping",
        BrowserShellPhase::HandoffCommitted => "handoff_committed",
        BrowserShellPhase::Rebinding => "rebinding",
        BrowserShellPhase::Ready => "ready",
    }
}

fn current_browser_shell_phase() -> BrowserShellPhase {
    BROWSER_SHELL_PHASE.with(|slot| slot.get())
}

async fn start_and_monitor_runtime_device_removal(
    controller: Arc<UiController>,
    device_id: String,
) -> Result<(), JsValue> {
    let app_core = controller.app_core().clone();
    let ceremony_handle = ceremony_workflows::start_device_removal_ceremony(&app_core, device_id)
        .await
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let status_handle = ceremony_handle.status_handle();
    match ceremony_workflows::get_key_rotation_ceremony_status(&app_core, &status_handle).await {
        Ok(status) if status.is_complete => {
            settings_workflows::refresh_settings_from_runtime(&app_core)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.request_rerender();
            return Ok(());
        }
        Ok(_) => {}
        Err(error) => {
            return Err(JsValue::from_str(&error.to_string()));
        }
    }
    shared_web_task_owner().spawn_local({
        let controller = controller.clone();
        let app_core = app_core.clone();
        async move {
            let lifecycle = ceremony_workflows::monitor_key_rotation_ceremony_with_policy(
                &app_core,
                &status_handle,
                ceremony_workflows::CeremonyPollPolicy {
                    interval: std::time::Duration::from_millis(250),
                    max_attempts: 160,
                    rollback_on_failure: true,
                    refresh_settings_on_complete: true,
                },
                |_| {
                    controller.request_rerender();
                },
                |duration| {
                    let app_core = app_core.clone();
                    async move {
                        let sleep_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                        let _ = time_workflows::sleep_ms(&app_core, sleep_ms).await;
                    }
                },
            )
            .await;

            match lifecycle {
                Ok(lifecycle) => {
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] device_removal_monitor state={:?};complete={};failed={};attempts={}",
                            lifecycle.state,
                            lifecycle.status.is_complete,
                            lifecycle.status.has_failed,
                            lifecycle.attempts
                        )
                        .into(),
                    );
                    controller.request_rerender();
                }
                Err(error) => {
                    web_sys::console::warn_1(
                        &format!(
                            "[web-harness] device_removal_monitor failed: {error}"
                        )
                        .into(),
                    );
                }
            }
        }
    });
    Ok(())
}

#[derive(Clone, Debug)]
pub enum BootstrapHandoff {
    InitialBootstrap,
    PendingAccountBootstrap {
        account_name: String,
        source: PendingAccountBootstrapSource,
    },
    RuntimeIdentityStaged {
        authority_id: AuthorityId,
        device_id: DeviceId,
        source: RuntimeIdentityStageSource,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum PendingAccountBootstrapSource {
    HarnessSemanticBridge,
    OnboardingUi,
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeIdentityStageSource {
    HarnessStaging,
    AuthoritySwitch,
    ImportDeviceEnrollment,
}

impl BootstrapHandoff {
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::InitialBootstrap => "initial_bootstrap".to_string(),
            Self::PendingAccountBootstrap {
                account_name,
                source,
            } => format!(
                "pending_account_bootstrap:{}:{}",
                match source {
                    PendingAccountBootstrapSource::HarnessSemanticBridge =>
                        "harness_semantic_bridge",
                    PendingAccountBootstrapSource::OnboardingUi => "onboarding_ui",
                },
                account_name
            ),
            Self::RuntimeIdentityStaged {
                authority_id,
                device_id,
                source,
            } => format!(
                "runtime_identity_staged:{}:{}:{}",
                match source {
                    RuntimeIdentityStageSource::HarnessStaging => "harness_staging",
                    RuntimeIdentityStageSource::AuthoritySwitch => "authority_switch",
                    RuntimeIdentityStageSource::ImportDeviceEnrollment =>
                        "import_device_enrollment",
                },
                authority_id,
                device_id
            ),
        }
    }
}

pub fn set_bootstrap_handoff_submitter(
    submitter: Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>,
) {
    BOOTSTRAP_HANDOFF_SUBMITTER.with(|slot| {
        *slot.borrow_mut() = Some(submitter);
    });
}

pub fn set_runtime_identity_stager(stager: Arc<dyn Fn(String) -> js_sys::Promise>) {
    RUNTIME_IDENTITY_STAGER.with(|slot| {
        *slot.borrow_mut() = Some(stager);
    });
}

pub async fn submit_bootstrap_handoff(handoff: BootstrapHandoff) -> Result<(), JsValue> {
    let detail = handoff.detail();
    web_sys::console::log_1(
        &format!("[web-harness] submit_bootstrap_handoff start detail={detail}").into(),
    );
    let submitter = BOOTSTRAP_HANDOFF_SUBMITTER.with(|slot| slot.borrow().clone());
    let submitter =
        submitter.ok_or_else(|| JsValue::from_str("bootstrap handoff submitter is unavailable"))?;
    let promise = submitter(handoff);
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await?;
    web_sys::console::log_1(
        &format!("[web-harness] submit_bootstrap_handoff done detail={detail}").into(),
    );
    Ok(())
}

pub async fn stage_runtime_identity(serialized_identity: String) -> Result<(), JsValue> {
    web_sys::console::log_1(&"[web-harness] stage_runtime_identity start".into());
    let stager = RUNTIME_IDENTITY_STAGER.with(|slot| slot.borrow().clone());
    let stager =
        stager.ok_or_else(|| JsValue::from_str("runtime identity stager is unavailable"))?;
    let promise = stager(serialized_identity);
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await?;
    web_sys::console::log_1(&"[web-harness] stage_runtime_identity done".into());
    Ok(())
}

fn browser_settings_section(section: SettingsSection) -> aura_ui::model::SettingsSection {
    match section {
        SettingsSection::Devices => aura_ui::model::SettingsSection::Devices,
    }
}

fn selected_channel_id(controller: &UiController) -> Result<ChannelId, JsValue> {
    let selected = controller
        .selected_channel_id()
        .ok_or_else(|| JsValue::from_str("no channel is selected"))?;
    selected
        .parse::<ChannelId>()
        .map_err(|error| JsValue::from_str(&format!("invalid selected channel id: {error}")))
}

async fn selected_channel_binding(controller: &UiController) -> Result<(String, String), JsValue> {
    let channel_id = selected_channel_id(controller)?;
    let context_id = {
        let core = controller.app_core().read().await;
        let chat = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        chat.channel(&channel_id)
            .and_then(|channel| channel.context_id)
    }
    .ok_or_else(|| {
        JsValue::from_str(&format!(
            "selected channel lacks authoritative context: {channel_id}"
        ))
    })?;

    Ok((channel_id.to_string(), context_id.to_string()))
}

fn semantic_channel_result(
    channel_id: String,
    context_id: Option<String>,
) -> SemanticCommandResponse {
    match context_id {
        Some(context_id) => {
            SemanticCommandResponse::accepted_authoritative_channel_binding(channel_id, context_id)
        }
        None => SemanticCommandResponse::accepted_channel_selection(channel_id),
    }
}

fn semantic_response_with_handle(
    handle: UiOperationHandle,
    value: SemanticCommandValue,
) -> SemanticCommandResponse {
    SemanticCommandResponse {
        submission: SubmissionState::Accepted,
        handle: SemanticSubmissionHandle {
            ui_operation: Some(handle),
        },
        value,
    }
}

fn semantic_unit_result_with_handle(handle: UiOperationHandle) -> SemanticCommandResponse {
    semantic_response_with_handle(handle, SemanticCommandValue::None)
}

fn semantic_channel_result_with_handle(
    handle: UiOperationHandle,
    channel_id: String,
    context_id: Option<String>,
) -> SemanticCommandResponse {
    match context_id {
        Some(context_id) => semantic_response_with_handle(
            handle,
            SemanticCommandValue::AuthoritativeChannelBinding {
                channel_id,
                context_id,
            },
        ),
        None => semantic_response_with_handle(
            handle,
            SemanticCommandValue::ChannelSelection { channel_id },
        ),
    }
}

fn begin_exact_ui_operation(
    controller: Arc<UiController>,
    operation_id: OperationId,
) -> UiOperationHandle {
    let instance_id = Rc::new(StdRefCell::new(None::<OperationInstanceId>));
    let captured_instance_id = instance_id.clone();
    let captured_operation_id = operation_id.clone();
    apply_browser_ui_mutation(controller, move |controller| {
        let next_instance_id =
            controller.begin_exact_operation_submission(captured_operation_id.clone());
        captured_instance_id.borrow_mut().replace(next_instance_id);
    });
    let instance_id = instance_id
        .borrow_mut()
        .take()
        .unwrap_or_else(|| panic!("exact browser operation submission must return an instance id"));
    UiOperationHandle::new(operation_id, instance_id)
}

fn spawn_background_semantic_task(
    label: &'static str,
    task: impl std::future::Future<Output = Result<(), JsValue>> + 'static,
) {
    shared_web_task_owner().spawn_local(async move {
        if let Err(error) = task.await {
            let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
            update_semantic_debug(label, Some(&detail));
            web_sys::console::error_1(
                &format!("[web-harness] background semantic task {label} failed: {detail}").into(),
            );
        }
    });
}

fn apply_handed_off_terminal_status(
    controller: Arc<UiController>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    terminal: Option<WorkflowTerminalStatus>,
) -> Result<(), JsValue> {
    let terminal = terminal.ok_or_else(|| {
        JsValue::from_str(&format!(
            "workflow handoff completed without terminal authoritative status for {}",
            operation_id.0
        ))
    })?;
    if terminal.status.kind != kind {
        return Err(JsValue::from_str(&format!(
            "workflow handoff returned mismatched semantic kind for {} (expected={kind:?} observed={:?})",
            operation_id.0, terminal.status.kind
        )));
    }
    if !terminal.status.phase.is_terminal() {
        return Err(JsValue::from_str(&format!(
            "workflow handoff returned non-terminal phase for {}: {:?}",
            operation_id.0, terminal.status.phase
        )));
    }
    apply_browser_ui_mutation(controller, move |controller| {
        controller.apply_authoritative_operation_status(operation_id, terminal.status);
    });
    Ok(())
}

fn selected_device_id(controller: &UiController) -> Result<String, JsValue> {
    let snapshot = controller.ui_snapshot();
    snapshot
        .selected_item_id(ListId::Devices)
        .map(str::to_string)
        .ok_or_else(|| JsValue::from_str("no device is selected"))
}

fn selected_authority_id(controller: &UiController) -> Option<String> {
    controller.selected_authority_id().map(|id| id.to_string())
}

pub(crate) fn publish_semantic_controller_snapshot(controller: Arc<UiController>) -> UiSnapshot {
    set_controller(controller.clone());
    let snapshot = controller.semantic_model_snapshot();
    controller.publish_ui_snapshot(snapshot.clone());
    web_sys::console::log_1(
        &format!(
            "[web-harness-publish] semantic screen={:?};readiness={:?};revision={:?}",
            snapshot.screen, snapshot.readiness, snapshot.revision
        )
        .into(),
    );
    snapshot
}

fn generation_js_value(generation_id: u64) -> JsValue {
    if generation_id == 0 {
        JsValue::NULL
    } else {
        JsValue::from_f64(generation_id as f64)
    }
}

fn sync_generation_globals(window: &web_sys::Window) {
    let active_generation = ACTIVE_GENERATION.with(|slot| slot.get());
    let ready_generation = READY_GENERATION.with(|slot| slot.get());
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_ACTIVE_GENERATION_KEY),
        &generation_js_value(active_generation),
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_READY_GENERATION_KEY),
        &generation_js_value(ready_generation),
    );
}

fn wake_page_owned_mutation_queues(window: &web_sys::Window) {
    for key in [
        "__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__",
        "__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__",
    ] {
        if let Ok(function) = Reflect::get(window.as_ref(), &JsValue::from_str(key))
            .and_then(|value| value.dyn_into::<Function>())
        {
            if let Err(error) = function.call0(window.as_ref()) {
                log_js_callback_error("page-owned mutation queue wake", &error);
            }
        }
    }
}

fn semantic_submit_surface_state() -> (&'static str, &'static str) {
    let active_generation = ACTIVE_GENERATION.with(|slot| slot.get());
    let has_controller = CONTROLLER.with(|slot| slot.borrow().is_some());
    let phase = current_browser_shell_phase();
    if active_generation == 0 {
        return ("unavailable", "semantic_submit_surface_missing_generation");
    }
    if !has_controller {
        return ("unavailable", "semantic_submit_surface_missing_controller");
    }
    match phase {
        BrowserShellPhase::Ready => ("ready", "semantic_submit_surface_ready"),
        BrowserShellPhase::Bootstrapping => {
            ("unavailable", "semantic_submit_surface_bootstrapping")
        }
        BrowserShellPhase::HandoffCommitted => {
            ("unavailable", "semantic_submit_surface_handoff_committed")
        }
        BrowserShellPhase::Rebinding => (
            "unavailable",
            "semantic_submit_surface_generation_rebinding",
        ),
    }
}

fn refresh_semantic_submit_surface(window: &web_sys::Window, binding_mode: &str) {
    let (status, detail) = semantic_submit_surface_state();
    web_sys::console::log_1(
        &format!(
            "[web-harness] semantic_submit_refresh status={status};detail={detail};generation={};phase={}",
            ACTIVE_GENERATION.with(|slot| slot.get()),
            browser_shell_phase_label(current_browser_shell_phase())
        )
        .into(),
    );
    publish_semantic_submit_state(window, status, detail, binding_mode);
}

pub fn set_browser_shell_phase(phase: BrowserShellPhase) {
    BROWSER_SHELL_PHASE.with(|slot| {
        slot.set(phase);
    });
    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_GENERATION_PHASE_KEY),
        &JsValue::from_str(browser_shell_phase_label(phase)),
    );
    refresh_semantic_submit_surface(&window, "semantic_bridge");
}

fn mark_generation_ready(generation_id: u64) {
    if generation_id == 0 {
        return;
    }
    READY_GENERATION.with(|slot| {
        if slot.get() < generation_id {
            slot.set(generation_id);
        }
    });
    GENERATION_READY_WAITERS.with(|slot| {
        let mut waiters = slot.borrow_mut();
        let mut pending = Vec::with_capacity(waiters.len());
        for (required_generation, sender) in waiters.drain(..) {
            if generation_id >= required_generation {
                let _ = sender.send(());
            } else {
                pending.push((required_generation, sender));
            }
        }
        *waiters = pending;
    });
    if let Some(window) = web_sys::window() {
        sync_generation_globals(&window);
    }
}

fn snapshot_marks_generation_ready(snapshot: &UiSnapshot) -> bool {
    snapshot.readiness == UiReadiness::Ready
}

pub fn set_active_generation(generation_id: u64) {
    ACTIVE_GENERATION.with(|slot| {
        slot.set(generation_id);
    });
    if let Some(window) = web_sys::window() {
        sync_generation_globals(&window);
        refresh_semantic_submit_surface(&window, "semantic_bridge");
    }
}

pub async fn wait_for_generation_ready(generation_id: u64) -> Result<(), JsValue> {
    if generation_id == 0 || READY_GENERATION.with(|slot| slot.get() >= generation_id) {
        return Ok(());
    }
    let (tx, rx) = oneshot::channel();
    GENERATION_READY_WAITERS.with(|slot| {
        slot.borrow_mut().push((generation_id, tx));
    });
    rx.await
        .map_err(|_| JsValue::from_str(&format!("generation_ready_wait_dropped:{generation_id}")))
}

pub(crate) fn schedule_browser_task_next_tick(
    action: impl FnOnce() + 'static,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let action = Rc::new(StdRefCell::new(Some(Box::new(action) as Box<dyn FnOnce()>)));
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 0)
        .map_err(|error| {
            JsValue::from_str(&format!("failed to schedule browser task: {error:?}"))
        })?;
    callback.forget();
    Ok(())
}

pub(crate) async fn schedule_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController) + 'static,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let (tx, rx) = oneshot::channel::<()>();
    let action = Rc::new(StdRefCell::new(Some(Box::new(move || {
        let snapshot = controller.semantic_model_snapshot();
        web_sys::console::log_1(
            &format!(
                "[web-ui-mutation] pre screen={:?};readiness={:?};focused={:?}",
                snapshot.screen, snapshot.readiness, snapshot.focused_control
            )
            .into(),
        );
        action(controller.as_ref());
        let final_snapshot = publish_semantic_controller_snapshot(controller.clone());
        web_sys::console::log_1(
            &format!(
                "[web-ui-mutation] post screen={:?};readiness={:?};focused={:?}",
                final_snapshot.screen, final_snapshot.readiness, final_snapshot.focused_control
            )
            .into(),
        );
    }) as Box<dyn FnOnce()>)));
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
        let _ = tx.send(());
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 0)
        .map_err(|error| {
            JsValue::from_str(&format!("failed to schedule UI mutation: {error:?}"))
        })?;
    callback.forget();
    rx.await
        .map_err(|_| JsValue::from_str("scheduled UI mutation dropped before execution"))?;
    Ok(())
}

pub(crate) fn apply_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController),
) {
    let snapshot = controller.semantic_model_snapshot();
    web_sys::console::log_1(
        &format!(
            "[web-ui-mutation] pre screen={:?};readiness={:?};focused={:?}",
            snapshot.screen, snapshot.readiness, snapshot.focused_control
        )
        .into(),
    );
    action(controller.as_ref());
    let final_snapshot = publish_semantic_controller_snapshot(controller);
    web_sys::console::log_1(
        &format!(
            "[web-ui-mutation] post screen={:?};readiness={:?};focused={:?}",
            final_snapshot.screen, final_snapshot.readiness, final_snapshot.focused_control
        )
        .into(),
    );
}

async fn submit_semantic_command(
    controller: Arc<UiController>,
    request: SemanticCommandRequest,
) -> Result<SemanticCommandResponse, JsValue> {
    match request.intent {
        IntentAction::OpenScreen(screen) => {
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_screen(screen);
            });
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateAccount { account_name } => {
            update_semantic_debug("create_account_begin", Some(&account_name));
            let staged_account_name = account_name.clone();
            schedule_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_account_setup_state(false, staged_account_name, None);
            })
            .await?;
            let stage_result =
                workflows::stage_account_creation(controller.app_core(), &account_name)
                    .await
                    .map_err(|error| JsValue::from_str(&error.user_message()))?;
            if stage_result.mode == AccountCreationStageMode::RuntimeInitialized {
                update_semantic_debug("create_account_runtime_path", Some(&account_name));
                controller.finalize_account_setup(ScreenId::Neighborhood);
                publish_semantic_controller_snapshot(controller);
            } else {
                update_semantic_debug("create_account_stage_start", Some(&account_name));
                web_sys::console::log_1(
                    &format!("[web-harness] create_account stage nickname={account_name}").into(),
                );
                update_semantic_debug("create_account_staged", Some(&account_name));
                web_sys::console::log_1(
                    &format!("[web-harness] create_account staged nickname={account_name}").into(),
                );
                update_semantic_debug("create_account_handoff_start", Some(&account_name));
                submit_bootstrap_handoff(BootstrapHandoff::PendingAccountBootstrap {
                    account_name: account_name.clone(),
                    source: PendingAccountBootstrapSource::HarnessSemanticBridge,
                })
                .await?;
                update_semantic_debug("create_account_handoff_done", Some(&account_name));
            }
            update_semantic_debug("create_account_return", Some(&account_name));
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateHome { home_name } => {
            context_workflows::create_home(controller.app_core(), Some(home_name), None)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateChannel { channel_name } => {
            let timestamp_ms = context_workflows::current_time_ms(controller.app_core())
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let created = messaging_workflows::create_channel_with_authoritative_binding(
                controller.app_core(),
                &channel_name,
                None,
                &[],
                1,
                timestamp_ms,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            Ok(semantic_channel_result(
                created.channel_id.to_string(),
                created.context_id.map(|context_id| context_id.to_string()),
            ))
        }
        IntentAction::StartDeviceEnrollment {
            device_name,
            invitee_authority_id,
            ..
        } => {
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_screen(ScreenId::Settings);
                controller.set_settings_section(browser_settings_section(SettingsSection::Devices));
            });
            let invitee_authority_id =
                invitee_authority_id
                    .parse::<AuthorityId>()
                    .map_err(|error| {
                        JsValue::from_str(&format!("invalid invitee authority id: {error}"))
                    })?;
            let start = ceremony_workflows::start_device_enrollment_ceremony(
                &controller.app_core(),
                device_name.clone(),
                invitee_authority_id,
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.write_clipboard(&start.enrollment_code);
            controller.push_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
                device_name: Some(device_name),
                code_len: Some(start.enrollment_code.len()),
                code: Some(start.enrollment_code),
            });
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::ImportDeviceEnrollmentCode { code } => {
            let app_core = controller.app_core().clone();
            let runtime = runtime_workflows::require_runtime(&app_core)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let storage_prefix = active_storage_prefix();
            let result = workflows::accept_device_enrollment_import(
                &app_core,
                DeviceEnrollmentImportRequest {
                    code: &code,
                    current_runtime_identity: CurrentRuntimeIdentity {
                        authority_id: runtime.authority_id(),
                        selected_runtime_identity: load_selected_runtime_identity(
                            &selected_runtime_identity_key(&storage_prefix),
                        )
                        .map_err(|error| JsValue::from_str(&error.to_string()))?,
                    },
                    storage_prefix: &storage_prefix,
                    rebootstrap_policy: RebootstrapPolicy::StageIfRequired,
                    operation: crate::WebUiOperation::ImportDeviceEnrollmentCode,
                },
            )
            .await
            .map_err(|error| JsValue::from_str(&error.user_message()))?;
            if result.rebootstrap_required {
                let staged_runtime_identity = result.staged_runtime_identity;
                submit_runtime_bootstrap_handoff(BootstrapHandoff::RuntimeIdentityStaged {
                    authority_id: staged_runtime_identity.authority_id,
                    device_id: staged_runtime_identity.device_id,
                    source: RuntimeIdentityStageSource::ImportDeviceEnrollment,
                })
                .await
                .map_err(|error| JsValue::from_str(&error.user_message()))?;
                return Ok(SemanticCommandResponse::accepted_without_value());
            }
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.finalize_account_setup(ScreenId::Neighborhood);
            });
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::OpenSettingsSection(section) => {
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_screen(ScreenId::Settings);
                controller.set_settings_section(browser_settings_section(section));
            });
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::RemoveSelectedDevice { device_id } => {
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_screen(ScreenId::Settings);
                controller.set_settings_section(browser_settings_section(SettingsSection::Devices));
            });
            let device_id = match device_id {
                Some(device_id) => device_id,
                None => selected_device_id(&controller)?,
            };
            start_and_monitor_runtime_device_removal(controller.clone(), device_id).await?;
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::SwitchAuthority { authority_id } => {
            apply_browser_ui_mutation(controller.clone(), move |controller| {
                controller.set_screen(ScreenId::Settings);
                controller.set_settings_section(aura_ui::model::SettingsSection::Authority);
            });
            if selected_authority_id(&controller).as_deref() == Some(authority_id.as_str()) {
                return Ok(SemanticCommandResponse::accepted_without_value());
            }
            let authority_id = authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            if !controller.request_authority_switch(authority_id) {
                return Err(JsValue::from_str(
                    "authority switching is not available for this frontend",
                ));
            }
            Ok(SemanticCommandResponse::accepted_without_value())
        }
        IntentAction::CreateContactInvitation {
            receiver_authority_id,
            ..
        } => {
            let handle =
                begin_exact_ui_operation(controller.clone(), OperationId::invitation_create());
            let authority_id = receiver_authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            let app_core = controller.app_core().clone();
            let invitation = invitation_workflows::create_contact_invitation_with_instance(
                &app_core,
                authority_id.clone(),
                None,
                None,
                None,
                Some(handle.instance_id().clone()),
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let code =
                invitation_workflows::export_invitation(&app_core, invitation.invitation_id())
                    .await
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
            controller.write_clipboard(&code);
            controller.push_runtime_fact(RuntimeFact::InvitationCodeReady {
                receiver_authority_id: Some(authority_id.to_string()),
                source_operation: aura_app::ui::contract::OperationId::invitation_create(),
                code: Some(code.clone()),
            });
            Ok(semantic_response_with_handle(
                handle,
                SemanticCommandValue::ContactInvitationCode { code },
            ))
        }
        IntentAction::AcceptContactInvitation { code } => {
            let app_core = controller.app_core().clone();
            let handle =
                begin_exact_ui_operation(controller.clone(), OperationId::invitation_accept());
            let invitation = invitation_workflows::import_invitation_details(&app_core, &code)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let invitation_info = invitation.info().clone();
            let instance_id = handle.instance_id().clone();
            let controller = controller.clone();
            spawn_background_semantic_task("accept_contact_invitation_failed", async move {
                let result = invitation_workflows::accept_imported_invitation_with_terminal_status(
                    &app_core,
                    invitation,
                    Some(instance_id),
                )
                .await
                .result;
                let terminal = match &result {
                    Ok(()) => {
                        if let InvitationBridgeType::Contact { nickname } =
                            &invitation_info.invitation_type
                        {
                            let display_name = nickname
                                .clone()
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| invitation_info.sender_id.to_string());
                            apply_browser_ui_mutation(controller.clone(), move |controller| {
                                controller.complete_runtime_contact_invitation_acceptance(
                                    invitation_info.sender_id,
                                    display_name,
                                );
                            });
                        }
                        WorkflowTerminalStatus {
                            causality: None,
                            status: SemanticOperationStatus::new(
                                SemanticOperationKind::AcceptContactInvitation,
                                SemanticOperationPhase::Succeeded,
                            ),
                        }
                    }
                    Err(error) => WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::failed(
                            SemanticOperationKind::AcceptContactInvitation,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Command,
                                SemanticFailureCode::InternalError,
                            )
                            .with_detail(error.to_string()),
                        ),
                    },
                };
                apply_handed_off_terminal_status(
                    controller,
                    OperationId::invitation_accept(),
                    SemanticOperationKind::AcceptContactInvitation,
                    Some(terminal),
                )?;
                result.map_err(|error| JsValue::from_str(&error.to_string()))
            });
            Ok(semantic_unit_result_with_handle(handle))
        }
        IntentAction::AcceptPendingChannelInvitation => {
            let handle =
                begin_exact_ui_operation(controller.clone(), OperationId::invitation_accept());
            let app_core = controller.app_core().clone();
            let instance_id = handle.instance_id().clone();
            let controller = controller.clone();
            spawn_background_semantic_task(
                "accept_pending_channel_invitation_failed",
                async move {
                    let result = invitation_workflows::accept_pending_channel_invitation_with_binding_terminal_status(
                        &app_core,
                        Some(instance_id),
                    )
                    .await
                    .result;
                    let terminal = match &result {
                        Ok(_) => WorkflowTerminalStatus {
                            causality: None,
                            status: SemanticOperationStatus::new(
                                SemanticOperationKind::AcceptPendingChannelInvitation,
                                SemanticOperationPhase::Succeeded,
                            ),
                        },
                        Err(error) => WorkflowTerminalStatus {
                            causality: None,
                            status: SemanticOperationStatus::failed(
                                SemanticOperationKind::AcceptPendingChannelInvitation,
                                SemanticOperationError::new(
                                    SemanticFailureDomain::Command,
                                    SemanticFailureCode::InternalError,
                                )
                                .with_detail(error.to_string()),
                            ),
                        },
                    };
                    apply_handed_off_terminal_status(
                        controller,
                        OperationId::invitation_accept(),
                        SemanticOperationKind::AcceptPendingChannelInvitation,
                        Some(terminal),
                    )?;
                    result
                        .map(|_| ())
                        .map_err(|error| JsValue::from_str(&error.to_string()))
                },
            );
            Ok(semantic_unit_result_with_handle(handle))
        }
        IntentAction::JoinChannel { channel_name } => {
            let handle = begin_exact_ui_operation(controller.clone(), OperationId::join_channel());
            messaging_workflows::join_channel_by_name_with_instance(
                controller.app_core(),
                &channel_name,
                Some(handle.instance_id().clone()),
            )
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let (channel_id, context_id) = selected_channel_binding(&controller).await?;
            Ok(semantic_channel_result_with_handle(
                handle,
                channel_id,
                Some(context_id),
            ))
        }
        IntentAction::InviteActorToChannel {
            authority_id,
            channel_id,
        } => {
            let handle =
                begin_exact_ui_operation(controller.clone(), OperationId::invitation_create());
            let authority_id = authority_id
                .parse::<AuthorityId>()
                .map_err(|error| JsValue::from_str(&format!("invalid authority id: {error}")))?;
            let channel_id = channel_id
                .ok_or_else(|| {
                    JsValue::from_str(
                        "invite_actor_to_channel requires an authoritative channel binding",
                    )
                })?
                .parse::<ChannelId>()
                .map_err(|error| JsValue::from_str(&format!("invalid channel id: {error}")))?;
            let app_core = controller.app_core().clone();
            let authority_id = authority_id.to_string();
            let channel_id = channel_id.to_string();
            let instance_id = handle.instance_id().clone();
            let controller = controller.clone();
            spawn_background_semantic_task("invite_actor_to_channel_failed", async move {
                let result =
                    messaging_workflows::invite_user_to_channel_with_context_terminal_status(
                        &app_core,
                        &authority_id,
                        &channel_id,
                        None,
                        Some(instance_id),
                        None,
                        None,
                    )
                    .await
                    .result;
                let terminal = match &result {
                    Ok(_) => WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::new(
                            SemanticOperationKind::InviteActorToChannel,
                            SemanticOperationPhase::Succeeded,
                        ),
                    },
                    Err(error) => WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::failed(
                            SemanticOperationKind::InviteActorToChannel,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Command,
                                SemanticFailureCode::InternalError,
                            )
                            .with_detail(error.to_string()),
                        ),
                    },
                };
                apply_handed_off_terminal_status(
                    controller,
                    OperationId::invitation_create(),
                    SemanticOperationKind::InviteActorToChannel,
                    Some(terminal),
                )?;
                result
                    .map(|_| ())
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            });
            Ok(semantic_unit_result_with_handle(handle))
        }
        IntentAction::SendChatMessage { message } => {
            let handle = begin_exact_ui_operation(controller.clone(), OperationId::send_message());
            let timestamp_ms = context_workflows::current_time_ms(controller.app_core())
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            let channel_id = selected_channel_id(&controller)?;
            let app_core = controller.app_core().clone();
            let instance_id = handle.instance_id().clone();
            let controller = controller.clone();
            spawn_background_semantic_task("send_chat_message_failed", async move {
                let result = messaging_workflows::send_message_with_instance(
                    &app_core,
                    channel_id,
                    &message,
                    timestamp_ms,
                    Some(instance_id),
                )
                .await;
                let terminal = match &result {
                    Ok(_) => WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::new(
                            SemanticOperationKind::SendChatMessage,
                            SemanticOperationPhase::Succeeded,
                        ),
                    },
                    Err(error) => WorkflowTerminalStatus {
                        causality: None,
                        status: SemanticOperationStatus::failed(
                            SemanticOperationKind::SendChatMessage,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Command,
                                SemanticFailureCode::InternalError,
                            )
                            .with_detail(error.to_string()),
                        ),
                    },
                };
                apply_handed_off_terminal_status(
                    controller,
                    OperationId::send_message(),
                    SemanticOperationKind::SendChatMessage,
                    Some(terminal),
                )?;
                result
                    .map(|_| ())
                    .map_err(|error| JsValue::from_str(&error.to_string()))
            });
            Ok(semantic_unit_result_with_handle(handle))
        }
    }
}

fn publish_ui_snapshot_now(
    window: &web_sys::Window,
    value: JsValue,
    json: String,
    screen: ScreenId,
    modal: Option<ModalId>,
    operation_count: usize,
) -> bool {
    let should_publish = LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        let mut last = slot.borrow_mut();
        if last.as_deref() == Some(json.as_str()) {
            false
        } else {
            *last = Some(json.clone());
            true
        }
    });
    if !should_publish {
        return false;
    }

    let cache_publish = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
        &value,
    );
    let json_publish = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_JSON__"),
        &JsValue::from_str(&json),
    );

    let mut publication_issues = Vec::new();
    let cache_published = match cache_publish {
        Ok(published) => published,
        Err(error) => {
            publication_issues.push(format!("cache_publish_failed: {}", js_value_detail(&error)));
            false
        }
    };
    let json_published = match json_publish {
        Ok(published) => published,
        Err(error) => {
            publication_issues.push(format!("json_publish_failed: {}", js_value_detail(&error)));
            false
        }
    };

    let (binding_mode, driver_push_published) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_UI_STATE"),
    )
    .ok()
    .and_then(|candidate| candidate.dyn_into::<Function>().ok())
    .map(|function| {
        if let Err(error) = function.call1(window.as_ref(), &JsValue::from_str(&json)) {
            publication_issues.push(format!("driver_push_failed: {}", js_value_detail(&error)));
            log_js_callback_error("driver UI state push", &error);
            ("driver_push", false)
        } else {
            ("driver_push", true)
        }
    })
    .unwrap_or(("window_cache_only", false));
    if binding_mode == "window_cache_only" {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-publish]binding={binding_mode};screen={screen:?};modal={modal:?};ops={operation_count}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-state]screen={screen:?};modal={modal:?};ops={operation_count};binding={binding_mode}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!("[aura-ui-json]{json}")));
    }

    let has_observable_publication = cache_published || json_published || driver_push_published;
    let (status, detail) = if !has_observable_publication {
        ("unavailable", publication_issues.join(" | "))
    } else if publication_issues.is_empty() {
        ("published", "published".to_string())
    } else {
        ("degraded", publication_issues.join(" | "))
    };
    update_publication_state(
        window,
        UI_PUBLICATION_STATE_KEY,
        "ui_state",
        status,
        &detail,
        binding_mode,
    );
    web_sys::console::log_1(
        &format!(
            "[web-harness-publish] ui_state status={status};binding={binding_mode};screen={screen:?};modal={modal:?};ops={operation_count}"
        )
        .into(),
    );

    has_observable_publication
}

fn publish_render_heartbeat(window: &web_sys::Window, heartbeat: &RenderHeartbeat) {
    let Ok(value) = to_value(heartbeat) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };

    let heartbeat_publish = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT__"),
        &value,
    );
    let heartbeat_json_publish = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_JSON__"),
        &JsValue::from_str(&json),
    );

    let mut publication_issues = Vec::new();
    let heartbeat_published = match heartbeat_publish {
        Ok(published) => published,
        Err(error) => {
            publication_issues.push(format!(
                "heartbeat_publish_failed: {}",
                js_value_detail(&error)
            ));
            false
        }
    };
    let heartbeat_json_published = match heartbeat_json_publish {
        Ok(published) => published,
        Err(error) => {
            publication_issues.push(format!(
                "heartbeat_json_publish_failed: {}",
                js_value_detail(&error)
            ));
            false
        }
    };

    let driver_push_published = if let Ok(function) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_RENDER_HEARTBEAT"),
    )
    .and_then(|candidate| candidate.dyn_into::<Function>())
    {
        if let Err(error) = function.call1(window.as_ref(), &JsValue::from_str(&json)) {
            publication_issues.push(format!("driver_push_failed: {}", js_value_detail(&error)));
            log_js_callback_error("driver render heartbeat push", &error);
            false
        } else {
            true
        }
    } else {
        false
    };

    let binding_mode = if driver_push_published {
        "driver_push"
    } else {
        "window_cache_only"
    };
    let has_observable_publication =
        heartbeat_published || heartbeat_json_published || driver_push_published;
    let (status, detail) = if !has_observable_publication {
        ("unavailable", publication_issues.join(" | "))
    } else if publication_issues.is_empty() {
        ("published", "published".to_string())
    } else {
        ("degraded", publication_issues.join(" | "))
    };
    update_publication_state(
        window,
        RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
        "render_heartbeat",
        status,
        &detail,
        binding_mode,
    );
}

fn js_value_detail(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            JSON::stringify(error)
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

fn update_publication_state(
    window: &web_sys::Window,
    key: &str,
    surface: &str,
    status: &str,
    detail: &str,
    binding_mode: &str,
) {
    let state = Object::new();
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("surface"),
        &JsValue::from_str(surface),
    );
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("status"),
        &JsValue::from_str(status),
    );
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("detail"),
        &JsValue::from_str(detail),
    );
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("binding_mode"),
        &JsValue::from_str(binding_mode),
    );
    if let Err(error) = Reflect::set(window.as_ref(), &JsValue::from_str(key), state.as_ref()) {
        web_sys::console::error_1(&JsValue::from_str(&format!(
            "[web-harness] failed to update publication state {key}: {}",
            js_value_detail(&error)
        )));
    }
}

fn publish_semantic_submit_state(
    window: &web_sys::Window,
    status: &str,
    detail: &str,
    binding_mode: &str,
) {
    update_publication_state(
        window,
        SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY,
        "semantic_submit",
        status,
        detail,
        binding_mode,
    );

    let Ok(state) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY),
    ) else {
        return;
    };
    if state.is_null() || state.is_undefined() {
        return;
    }

    let generation_id = ACTIVE_GENERATION.with(|slot| slot.get());
    let phase = Reflect::get(window.as_ref(), &JsValue::from_str(UI_GENERATION_PHASE_KEY))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| browser_shell_phase_label(BrowserShellPhase::Rebinding).to_string());
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("generation_id"),
        &generation_js_value(generation_id),
    );
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("phase"),
        &JsValue::from_str(&phase),
    );
    let enqueue_ready = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_SEMANTIC_ENQUEUE__"),
    )
    .ok()
    .and_then(|candidate| candidate.dyn_into::<Function>().ok())
    .is_some();
    let _ = Reflect::set(
        &state,
        &JsValue::from_str("enqueue_ready"),
        &JsValue::from_bool(enqueue_ready),
    );
    if let Ok(function) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE"),
    )
    .and_then(|candidate| candidate.dyn_into::<Function>())
    {
        if let Err(error) = function.call1(window.as_ref(), &state) {
            log_js_callback_error("driver semantic submit state push", &error);
        }
    }
    wake_page_owned_mutation_queues(window);
}

fn log_js_callback_error(context: &str, error: &JsValue) {
    let detail = js_value_detail(error);
    web_sys::console::error_1(&JsValue::from_str(&format!(
        "[web-harness] {context} failed: {detail}"
    )));
}

pub fn set_controller(controller: Arc<UiController>) {
    let controller_changed = CONTROLLER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let changed = slot
            .as_ref()
            .map(|current| !Arc::ptr_eq(current, &controller))
            .unwrap_or(true);
        *slot = Some(controller);
        changed
    });
    if controller_changed {
        LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
    if let Some(window) = web_sys::window() {
        refresh_semantic_submit_surface(&window, "semantic_bridge");
    }
}

pub fn clear_controller(reason: &str) {
    CONTROLLER.with(|slot| {
        *slot.borrow_mut() = None;
    });
    LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        *slot.borrow_mut() = None;
    });
    RENDER_SEQ.with(|slot| {
        *slot.borrow_mut() = 0;
    });
    ACTIVE_GENERATION.with(|slot| {
        slot.set(0);
    });
    set_browser_shell_phase(BrowserShellPhase::Rebinding);
    if let Some(window) = web_sys::window() {
        let window_ref = window.as_ref();
        let _ = Reflect::set(
            window_ref,
            &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
            &JsValue::NULL,
        );
        let _ = Reflect::set(
            window_ref,
            &JsValue::from_str("__AURA_UI_STATE_JSON__"),
            &JsValue::NULL,
        );
        sync_generation_globals(&window);
        update_publication_state(
            &window,
            UI_PUBLICATION_STATE_KEY,
            "ui_state",
            "unavailable",
            reason,
            "generation_rebinding",
        );
        update_publication_state(
            &window,
            RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
            "render_heartbeat",
            "unavailable",
            reason,
            "generation_rebinding",
        );
        publish_semantic_submit_state(&window, "unavailable", reason, "generation_rebinding");
    }
}

fn current_controller() -> Result<Arc<UiController>, JsValue> {
    CONTROLLER
        .with(|slot| slot.borrow().clone())
        .ok_or_else(|| JsValue::from_str("Runtime bridge not available"))
}

pub fn publish_ui_snapshot(snapshot: &UiSnapshot) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let render_seq = RENDER_SEQ.with(|slot| {
        let mut seq = slot.borrow_mut();
        *seq = seq.saturating_add(1);
        *seq
    });
    let mut snapshot = snapshot.clone();
    snapshot.revision.render_seq = Some(render_seq);
    let Ok(value) = to_value(&snapshot) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };
    let screen = snapshot.screen;
    let open_modal = snapshot.open_modal;
    let operation_count = snapshot.operations.len();
    if !publish_ui_snapshot_now(&window, value, json, screen, open_modal, operation_count) {
        return;
    }
    let generation_id = ACTIVE_GENERATION.with(|slot| slot.get());
    if snapshot_marks_generation_ready(&snapshot) {
        mark_generation_ready(generation_id);
    }

    let callback_window = window.clone();
    let callback = Closure::once_into_js(move || {
        publish_render_heartbeat(
            &callback_window,
            &RenderHeartbeat {
                screen,
                open_modal,
                render_seq,
            },
        );
    });
    let callback_fn: &js_sys::Function = callback.unchecked_ref();
    if let Err(error) = window.request_animation_frame(callback_fn) {
        log_js_callback_error("requestAnimationFrame publish_ui_snapshot", &error);
        update_publication_state(
            &window,
            RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
            "render_heartbeat",
            "unavailable",
            &format!(
                "request_animation_frame_failed: {}",
                js_value_detail(&error)
            ),
            "driver_push",
        );
    }
}

fn published_ui_snapshot_value(window: &web_sys::Window) -> JsValue {
    let published_json = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_JSON__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined());

    if let Some(published_json) = published_json {
        if published_json.as_string().is_some() {
            return published_json;
        }
    }

    let published_cache = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined());
    if let Some(published_cache) = published_cache {
        return published_cache;
    }

    update_publication_state(
        window,
        UI_PUBLICATION_STATE_KEY,
        "ui_state",
        "unavailable",
        "semantic_snapshot_not_published",
        "observation_only",
    );
    JsValue::NULL
}

fn update_semantic_debug(event: &str, detail: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let debug = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
    )
    .ok()
    .filter(|value| !value.is_null() && !value.is_undefined())
    .unwrap_or_else(|| {
        let object = Object::new();
        let _ = Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__AURA_SEMANTIC_DEBUG__"),
            object.as_ref(),
        );
        object.into()
    });
    let _ = Reflect::set(
        &debug,
        &JsValue::from_str("last_event"),
        &JsValue::from_str(event),
    );
    let _ = Reflect::set(
        &debug,
        &JsValue::from_str("last_detail"),
        &detail.map(JsValue::from_str).unwrap_or(JsValue::NULL),
    );
}

fn install_page_owned_mutation_queues(window: &web_sys::Window) -> Result<(), JsValue> {
    let installer = Function::new_no_args(
        r#"
const window = globalThis;
if (window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED) {
  window.__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE?.(
    window.__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__ ?? null,
  );
  window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__?.();
  window.__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__?.();
  return true;
}

window.__AURA_DRIVER_PENDING_NAV_SCREEN__ =
  window.__AURA_DRIVER_PENDING_NAV_SCREEN__ ?? null;
window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__ =
  Array.isArray(window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__)
    ? window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__
    : [];
window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__ =
  Array.isArray(window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__)
    ? window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__
    : [];
window.__AURA_DRIVER_SEMANTIC_QUEUE__ =
  window.__AURA_DRIVER_SEMANTIC_QUEUE__ ?? [];
window.__AURA_DRIVER_SEMANTIC_RESULTS__ =
  window.__AURA_DRIVER_SEMANTIC_RESULTS__ ?? Object.create(null);
window.__AURA_DRIVER_SEMANTIC_BUSY__ =
  window.__AURA_DRIVER_SEMANTIC_BUSY__ ?? false;
window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ =
  window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ ?? false;
window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__ =
  window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__ ?? [];
window.__AURA_DRIVER_RUNTIME_STAGE_RESULTS__ =
  window.__AURA_DRIVER_RUNTIME_STAGE_RESULTS__ ?? Object.create(null);
window.__AURA_DRIVER_RUNTIME_STAGE_BUSY__ =
  window.__AURA_DRIVER_RUNTIME_STAGE_BUSY__ ?? false;
window.__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__ =
  window.__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__ ?? false;
window.__AURA_DRIVER_SEMANTIC_DEBUG__ =
  window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? {
    last_event: "installed",
    active_command_id: null,
    last_error: null,
    queue_depth: 0,
    last_progress_at: Date.now(),
  };
window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__ =
  window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__ ?? {
    last_event: "installed",
    active_command_id: null,
    last_error: null,
    queue_depth: 0,
    last_progress_at: Date.now(),
  };

window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__ = (delayMs = 0) => {
  const effectiveDelay =
    Number.isFinite(delayMs) && delayMs >= 0 ? Number(delayMs) : 0;
  if (effectiveDelay === 0) {
    if (Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)) {
      console.log(
        `[driver-page] semantic queue wake depth=${window.__AURA_DRIVER_SEMANTIC_QUEUE__.length}`,
      );
    }
    queueMicrotask(() => {
      window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__?.();
    });
    return;
  }
  if (window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__) {
    return;
  }
  window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ = true;
  window.setTimeout(() => {
    window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ = false;
    if (Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)) {
      console.log(
        `[driver-page] semantic queue wake depth=${window.__AURA_DRIVER_SEMANTIC_QUEUE__.length}`,
      );
    }
    window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__?.();
  }, effectiveDelay);
};

window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__ = () => {
  const semanticQueue = window.__AURA_DRIVER_SEMANTIC_QUEUE__;
  const semanticResults = window.__AURA_DRIVER_SEMANTIC_RESULTS__;
  const semanticDebug = window.__AURA_DRIVER_SEMANTIC_DEBUG__;
  if (semanticDebug) {
    semanticDebug.queue_depth = Array.isArray(semanticQueue)
      ? semanticQueue.length
      : 0;
  }
  if (Array.isArray(semanticQueue) && semanticQueue.length > 0) {
    console.log(
      `[driver-page] semantic queue pump depth=${semanticQueue.length};busy=${window.__AURA_DRIVER_SEMANTIC_BUSY__ === true}`,
    );
  }
  if (
    window.__AURA_DRIVER_SEMANTIC_BUSY__ ||
    !Array.isArray(semanticQueue) ||
    semanticQueue.length === 0
  ) {
    return;
  }
  const harness = window.__AURA_HARNESS__;
  const submitState = window.__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__ ?? null;
  const submitReady = submitState?.status === "ready";
  if (!submitReady || typeof harness?.submit_semantic_command !== "function") {
    const waitEvent = submitReady ? "queue_wait_bridge" : "queue_wait_ready";
    if (semanticDebug?.last_event !== waitEvent) {
      console.log(
        `[driver-page] semantic queue wait event=${waitEvent};submit_ready=${submitReady};has_bridge=${typeof harness?.submit_semantic_command === "function"}`,
      );
    }
    if (semanticDebug) {
      semanticDebug.last_event = waitEvent;
      semanticDebug.last_error = null;
      semanticDebug.active_command_id = null;
      semanticDebug.last_progress_at = Date.now();
    }
    window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__?.(25);
    return;
  }
  const nextJson = semanticQueue.shift();
  if (typeof nextJson !== "string" || nextJson.length === 0) {
    queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
    return;
  }
  let next = null;
  try {
    next = JSON.parse(nextJson);
  } catch (error) {
    if (semanticDebug) {
      semanticDebug.last_event = "queue_parse_error";
      semanticDebug.last_error = error?.message ?? String(error);
      semanticDebug.active_command_id = null;
      semanticDebug.last_progress_at = Date.now();
    }
    queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
    return;
  }
  if (!next || typeof next.command_id !== "string") {
    queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
    return;
  }
  if (semanticDebug) {
    semanticDebug.last_event = "queue_start";
    semanticDebug.active_command_id = next.command_id;
    semanticDebug.last_error = null;
    semanticDebug.last_progress_at = Date.now();
  }
  window.__AURA_DRIVER_SEMANTIC_BUSY__ = true;
  Promise.resolve()
    .then(() => {
      console.log(`[driver-page] semantic queue start id=${next.command_id}`);
      const requestObject = JSON.parse(next.request_json);
      if (semanticDebug) {
        semanticDebug.last_event = "queue_invoke";
        semanticDebug.last_progress_at = Date.now();
      }
      return harness.submit_semantic_command(requestObject);
    })
    .then((result) => {
      console.log(`[driver-page] semantic queue ok id=${next.command_id}`);
      semanticResults[next.command_id] = { ok: true, result };
      Promise.resolve(
        window.__AURA_DRIVER_PUSH_SEMANTIC_RESULT?.({
          command_id: next.command_id,
          ok: true,
          result,
        }),
      ).catch(() => {});
      if (semanticDebug) {
        semanticDebug.last_event = "queue_ok";
        semanticDebug.active_command_id = null;
        semanticDebug.last_progress_at = Date.now();
      }
    })
    .catch((error) => {
      console.error(
        `[driver-page] semantic queue error id=${next.command_id}: ${error?.message ?? String(error)}`,
      );
      semanticResults[next.command_id] = {
        ok: false,
        error: error?.message ?? String(error),
      };
      Promise.resolve(
        window.__AURA_DRIVER_PUSH_SEMANTIC_RESULT?.({
          command_id: next.command_id,
          ok: false,
          error: error?.message ?? String(error),
        }),
      ).catch(() => {});
      if (semanticDebug) {
        semanticDebug.last_event = "queue_error";
        semanticDebug.last_error = error?.message ?? String(error);
        semanticDebug.active_command_id = null;
        semanticDebug.last_progress_at = Date.now();
      }
    })
    .finally(() => {
      window.__AURA_DRIVER_SEMANTIC_BUSY__ = false;
      if (semanticDebug) {
        semanticDebug.queue_depth = Array.isArray(semanticQueue)
          ? semanticQueue.length
          : 0;
      }
      queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
    });
};

window.__AURA_DRIVER_SEMANTIC_ENQUEUE__ = (payloadJson) => {
  if (typeof payloadJson !== "string") {
    return {
      queue_depth: Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)
        ? window.__AURA_DRIVER_SEMANTIC_QUEUE__.length
        : null,
      debug: window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? null,
    };
  }
  if (!Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)) {
    throw new Error("window.__AURA_DRIVER_SEMANTIC_QUEUE__ is unavailable");
  }
  window.__AURA_DRIVER_SEMANTIC_QUEUE__.push(payloadJson);
  if (window.__AURA_DRIVER_SEMANTIC_DEBUG__) {
    window.__AURA_DRIVER_SEMANTIC_DEBUG__.last_event = "enqueued";
    window.__AURA_DRIVER_SEMANTIC_DEBUG__.active_command_id = null;
    window.__AURA_DRIVER_SEMANTIC_DEBUG__.queue_depth =
      window.__AURA_DRIVER_SEMANTIC_QUEUE__.length;
    window.__AURA_DRIVER_SEMANTIC_DEBUG__.last_progress_at = Date.now();
  }
  window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__?.();
  return {
    queue_depth: window.__AURA_DRIVER_SEMANTIC_QUEUE__.length,
    debug: window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? null,
  };
};

if (
  Array.isArray(window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__) &&
  window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__.length > 0
) {
  const seededCount = window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__.length;
  for (const payloadJson of window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__) {
    if (typeof payloadJson === "string" && payloadJson.length > 0) {
      window.__AURA_DRIVER_SEMANTIC_QUEUE__.push(payloadJson);
    }
  }
  console.log(`[driver-page] semantic queue seed count=${seededCount}`);
  window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__ = [];
}

if (
  Array.isArray(window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__) &&
  window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__.length > 0
) {
  const seededCount = window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__.length;
  for (const payloadJson of window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__) {
    if (typeof payloadJson === "string" && payloadJson.length > 0) {
      window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__.push(payloadJson);
    }
  }
  console.log(`[driver-page] runtime stage queue seed count=${seededCount}`);
  window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__ = [];
}

window.__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__ = (delayMs = 0) => {
  const effectiveDelay =
    Number.isFinite(delayMs) && delayMs >= 0 ? Number(delayMs) : 0;
  if (effectiveDelay === 0) {
    queueMicrotask(() => {
      window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__?.();
    });
    return;
  }
  if (window.__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__) {
    return;
  }
  window.__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__ = true;
  window.setTimeout(() => {
    window.__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__ = false;
    window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__?.();
  }, effectiveDelay);
};

window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__ = () => {
  const harness = window.__AURA_HARNESS__;
  const runtimeStageQueue = window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__;
  const runtimeStageResults = window.__AURA_DRIVER_RUNTIME_STAGE_RESULTS__;
  const runtimeStageDebug = window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__;
  if (runtimeStageDebug) {
    runtimeStageDebug.queue_depth = Array.isArray(runtimeStageQueue)
      ? runtimeStageQueue.length
      : 0;
  }
  if (
    window.__AURA_DRIVER_RUNTIME_STAGE_BUSY__ ||
    !Array.isArray(runtimeStageQueue) ||
    runtimeStageQueue.length === 0
  ) {
    return;
  }
  if (typeof harness?.stage_runtime_identity !== "function") {
    if (runtimeStageDebug) {
      runtimeStageDebug.last_event = "queue_wait_bridge";
      runtimeStageDebug.last_error = null;
      runtimeStageDebug.active_command_id = null;
      runtimeStageDebug.last_progress_at = Date.now();
    }
    window.__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__?.(25);
    return;
  }
  const nextJson = runtimeStageQueue.shift();
  if (typeof nextJson !== "string" || nextJson.length === 0) {
    queueMicrotask(window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__);
    return;
  }
  let next = null;
  try {
    next = JSON.parse(nextJson);
  } catch (error) {
    if (runtimeStageDebug) {
      runtimeStageDebug.last_event = "queue_parse_error";
      runtimeStageDebug.last_error = error?.message ?? String(error);
      runtimeStageDebug.active_command_id = null;
      runtimeStageDebug.last_progress_at = Date.now();
    }
    queueMicrotask(window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__);
    return;
  }
  if (!next || typeof next.command_id !== "string") {
    queueMicrotask(window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__);
    return;
  }
  if (runtimeStageDebug) {
    runtimeStageDebug.last_event = "queue_start";
    runtimeStageDebug.active_command_id = next.command_id;
    runtimeStageDebug.last_error = null;
    runtimeStageDebug.last_progress_at = Date.now();
  }
  window.__AURA_DRIVER_RUNTIME_STAGE_BUSY__ = true;
  Promise.resolve()
    .then(() => {
      console.log(`[driver-page] runtime stage queue start id=${next.command_id}`);
      if (runtimeStageDebug) {
        runtimeStageDebug.last_event = "queue_invoke";
        runtimeStageDebug.last_progress_at = Date.now();
      }
      return harness.stage_runtime_identity(next.runtime_identity_json);
    })
    .then((result) => {
      console.log(`[driver-page] runtime stage queue ok id=${next.command_id}`);
      runtimeStageResults[next.command_id] = { ok: true, result };
      Promise.resolve(
        window.__AURA_DRIVER_PUSH_RUNTIME_STAGE_RESULT?.({
          command_id: next.command_id,
          ok: true,
          result,
        }),
      ).catch(() => {});
      if (runtimeStageDebug) {
        runtimeStageDebug.last_event = "queue_ok";
        runtimeStageDebug.active_command_id = null;
        runtimeStageDebug.last_progress_at = Date.now();
      }
    })
    .catch((error) => {
      console.error(
        `[driver-page] runtime stage queue error id=${next.command_id}: ${error?.message ?? String(error)}`,
      );
      runtimeStageResults[next.command_id] = {
        ok: false,
        error: error?.message ?? String(error),
      };
      Promise.resolve(
        window.__AURA_DRIVER_PUSH_RUNTIME_STAGE_RESULT?.({
          command_id: next.command_id,
          ok: false,
          error: error?.message ?? String(error),
        }),
      ).catch(() => {});
      if (runtimeStageDebug) {
        runtimeStageDebug.last_event = "queue_error";
        runtimeStageDebug.last_error = error?.message ?? String(error);
        runtimeStageDebug.active_command_id = null;
        runtimeStageDebug.last_progress_at = Date.now();
      }
    })
    .finally(() => {
      window.__AURA_DRIVER_RUNTIME_STAGE_BUSY__ = false;
      if (runtimeStageDebug) {
        runtimeStageDebug.queue_depth = Array.isArray(runtimeStageQueue)
          ? runtimeStageQueue.length
          : 0;
      }
      queueMicrotask(window.__AURA_DRIVER_RUN_RUNTIME_STAGE_QUEUE__);
    });
};

window.__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE__ = (payloadJson) => {
  if (typeof payloadJson !== "string") {
    return {
      queue_depth: Array.isArray(window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__)
        ? window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__.length
        : null,
      debug: window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__ ?? null,
    };
  }
  if (!Array.isArray(window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__)) {
    throw new Error("window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__ is unavailable");
  }
  window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__.push(payloadJson);
  if (window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__) {
    window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__.last_event = "enqueued";
    window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__.active_command_id = null;
    window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__.queue_depth =
      window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__.length;
    window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__.last_progress_at = Date.now();
  }
  window.__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__?.();
  return {
    queue_depth: window.__AURA_DRIVER_RUNTIME_STAGE_QUEUE__.length,
    debug: window.__AURA_DRIVER_RUNTIME_STAGE_DEBUG__ ?? null,
  };
};

window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED = true;
window.__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE?.(
  window.__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__ ?? null,
);
window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__?.();
window.__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__?.();
return true;
"#,
    );
    installer.call0(window.as_ref()).map(|_| ())
}

pub fn install_window_harness_api() -> Result<(), JsValue> {
    let harness = Object::new();
    let observe = Object::new();

    let send_keys = Closure::wrap(Box::new(move |keys: JsValue| -> JsValue {
        if let Some(text) = keys.as_string() {
            if let Ok(controller) = current_controller() {
                controller.send_keys(&text);
            }
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_keys"),
        send_keys.as_ref().unchecked_ref(),
    )?;
    send_keys.forget();

    let send_key = Closure::wrap(Box::new(move |key: JsValue, repeat: JsValue| -> JsValue {
        let key_name = key.as_string().unwrap_or_default();
        let repeat = repeat
            .as_f64()
            .map(|value| value.max(1.0) as u16)
            .unwrap_or(1);
        if let Ok(controller) = current_controller() {
            controller.send_key_named(&key_name, repeat);
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_key"),
        send_key.as_ref().unchecked_ref(),
    )?;
    send_key.forget();

    let stage_runtime_identity_fn = Closure::wrap(Box::new(
        move |serialized_identity: JsValue| -> js_sys::Promise {
            let serialized_identity = serialized_identity
                .as_string()
                .ok_or_else(|| JsValue::from_str("runtime identity payload must be a string"));
            future_to_promise(async move {
                let serialized_identity = serialized_identity?;
                let _ = serde_json::from_str::<BootstrapRuntimeIdentity>(&serialized_identity)
                    .map_err(|error| {
                        JsValue::from_str(&format!(
                            "invalid staged runtime identity payload: {error}"
                        ))
                    })?;
                stage_runtime_identity(serialized_identity).await?;
                Ok(JsValue::UNDEFINED)
            })
        },
    )
        as Box<dyn FnMut(JsValue) -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("stage_runtime_identity"),
        stage_runtime_identity_fn.as_ref().unchecked_ref(),
    )?;
    stage_runtime_identity_fn.forget();

    let navigate_screen = Closure::wrap(Box::new(move |screen: JsValue| -> JsValue {
        let Some(screen_name) = screen.as_string() else {
            return JsValue::FALSE;
        };
        let Some(target) = classify_screen_item_id(&screen_name) else {
            return JsValue::FALSE;
        };
        if let Ok(controller) = current_controller() {
            shared_web_task_owner().spawn_local(async move {
                if let Err(error) = schedule_browser_ui_mutation(controller, move |controller| {
                    controller.set_screen(target);
                })
                .await
                {
                    web_sys::console::error_1(
                        &format!("[web-harness] scheduled UI mutation failed: {error:?}").into(),
                    );
                }
            });
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("navigate_screen"),
        navigate_screen.as_ref().unchecked_ref(),
    )?;
    navigate_screen.forget();

    let open_settings_section = Closure::wrap(Box::new(move |section: JsValue| -> JsValue {
        let Some(section_name) = section.as_string() else {
            return JsValue::FALSE;
        };
        let Some(target) = classify_semantic_settings_section_item_id(&section_name) else {
            return JsValue::FALSE;
        };
        if let Ok(controller) = current_controller() {
            shared_web_task_owner().spawn_local(async move {
                if let Err(error) = schedule_browser_ui_mutation(controller, move |controller| {
                    controller.set_screen(ScreenId::Settings);
                    controller.set_settings_section(browser_settings_section(target));
                })
                .await
                {
                    web_sys::console::error_1(
                        &format!("[web-harness] scheduled UI mutation failed: {error:?}").into(),
                    );
                }
            });
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("open_settings_section"),
        open_settings_section.as_ref().unchecked_ref(),
    )?;
    open_settings_section.forget();

    let snapshot = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let rendered = controller.snapshot();
        let payload = Object::new();
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("screen"),
            &JsValue::from_str(&rendered.screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("raw_screen"),
            &JsValue::from_str(&rendered.raw_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("authoritative_screen"),
            &JsValue::from_str(&rendered.authoritative_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("normalized_screen"),
            &JsValue::from_str(&rendered.normalized_screen),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("capture_consistency"),
            &JsValue::from_str("settled"),
        );
        payload.into()
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("snapshot"),
        snapshot.as_ref().unchecked_ref(),
    )?;
    snapshot.forget();

    let ui_state = Closure::wrap(Box::new(move || -> JsValue {
        if current_controller().is_err() {
            return JsValue::NULL;
        }
        let Some(window) = web_sys::window() else {
            return JsValue::NULL;
        };
        published_ui_snapshot_value(&window)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("ui_state"),
        ui_state.as_ref().unchecked_ref(),
    )?;
    ui_state.forget();

    let read_clipboard = Closure::wrap(Box::new(move || -> JsValue {
        if let Some(window) = web_sys::window() {
            if let Ok(value) = Reflect::get(
                window.as_ref(),
                &JsValue::from_str("__AURA_HARNESS_CLIPBOARD__"),
            ) {
                if let Some(text) = value.as_string() {
                    if !text.is_empty() {
                        return JsValue::from_str(&text);
                    }
                }
            }
        }
        match current_controller() {
            Ok(controller) => JsValue::from_str(&controller.read_clipboard()),
            Err(_) => JsValue::from_str(""),
        }
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("read_clipboard"),
        read_clipboard.as_ref().unchecked_ref(),
    )?;
    read_clipboard.forget();

    let submit_semantic_command_raw =
        Closure::wrap(Box::new(move |request_json: String| -> js_sys::Promise {
            future_to_promise(async move {
                update_semantic_debug("raw_entry", None);
                web_sys::console::log_1(&"[web-harness] submit_semantic_command entry".into());
                let outcome: Result<JsValue, JsValue> = async {
                    let controller = current_controller()?;
                    let request =
                        from_str::<SemanticCommandRequest>(&request_json).map_err(|error| {
                            JsValue::from_str(&format!("invalid semantic command request: {error}"))
                        })?;
                    update_semantic_debug("raw_parsed", Some(&format!("{:?}", request.intent)));
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] submit_semantic_command intent={:?}",
                            request.intent
                        )
                        .into(),
                    );
                    let response = submit_semantic_command(controller, request).await?;
                    web_sys::console::log_1(&"[web-harness] submit_semantic_command done".into());
                    to_string(&response)
                        .map(|response_json| JsValue::from_str(&response_json))
                        .map_err(|error| {
                            JsValue::from_str(&format!(
                                "failed to serialize semantic command response: {error}"
                            ))
                        })
                }
                .await;

                match outcome {
                    Ok(value) => {
                        update_semantic_debug("raw_resolved", None);
                        Ok(value)
                    }
                    Err(error) => {
                        update_semantic_debug("raw_rejected", error.as_string().as_deref());
                        Err(error)
                    }
                }
            })
        }) as Box<dyn FnMut(String) -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("__submit_semantic_command_raw"),
        submit_semantic_command_raw.as_ref().unchecked_ref(),
    )?;
    let submit_semantic_command_fn = Function::new_with_args(
        "request",
        r#"
window.__AURA_SEMANTIC_DEBUG__ = window.__AURA_SEMANTIC_DEBUG__ || {};
window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_entry";
console.log("[web-harness-js] submit_semantic_command wrapper entry");
const raw = window.__AURA_HARNESS__?.__submit_semantic_command_raw;
if (typeof raw !== "function") {
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_missing_raw";
  return Promise.reject(
    new Error("window.__AURA_HARNESS__.__submit_semantic_command_raw is unavailable"),
  );
}
try {
  const result = raw(JSON.stringify(request));
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_raw_return";
  console.log("[web-harness-js] submit_semantic_command wrapper raw returned");
  return Promise.resolve(result);
} catch (error) {
  window.__AURA_SEMANTIC_DEBUG__.last_event = "wrapper_threw";
  window.__AURA_SEMANTIC_DEBUG__.last_detail = error?.message ?? String(error);
  console.error("[web-harness-js] submit_semantic_command wrapper threw", error);
  return Promise.reject(error);
}
"#,
    );
    Reflect::set(
        &harness,
        &JsValue::from_str("submit_semantic_command"),
        submit_semantic_command_fn.as_ref(),
    )?;
    submit_semantic_command_raw.forget();

    let get_authority_id = Closure::wrap(Box::new(move || -> JsValue {
        match current_controller() {
            Ok(controller) => JsValue::from_str(&controller.authority_id()),
            Err(error) => error,
        }
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("get_authority_id"),
        get_authority_id.as_ref().unchecked_ref(),
    )?;
    get_authority_id.forget();

    let tail_log = Closure::wrap(Box::new(move |lines: JsValue| -> JsValue {
        let lines = lines
            .as_f64()
            .map(|value| value.max(1.0) as usize)
            .unwrap_or(20);
        let array = Array::new();
        if let Ok(controller) = current_controller() {
            for line in controller.tail_log(lines) {
                array.push(&JsValue::from_str(&line));
            }
        }
        array.into()
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("tail_log"),
        tail_log.as_ref().unchecked_ref(),
    )?;
    tail_log.forget();

    let root_structure = Closure::wrap(Box::new(move || -> JsValue {
        let Ok(controller) = current_controller() else {
            return JsValue::NULL;
        };
        let snapshot = controller.ui_snapshot();
        let Some(window) = web_sys::window() else {
            return JsValue::NULL;
        };
        let Some(document) = window.document() else {
            return JsValue::NULL;
        };

        let payload = Object::new();
        let app_root_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::AppRoot
                .web_dom_id()
                .expect("ControlId::AppRoot must define a web DOM id")
        );
        let modal_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ModalRegion
                .web_dom_id()
                .expect("ControlId::ModalRegion must define a web DOM id")
        );
        let onboarding_root_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::OnboardingRoot
                .web_dom_id()
                .expect("ControlId::OnboardingRoot must define a web DOM id")
        );
        let toast_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ToastRegion
                .web_dom_id()
                .expect("ControlId::ToastRegion must define a web DOM id")
        );
        let screen_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::Screen(snapshot.screen)
                .web_dom_id()
                .expect("ControlId::Screen(snapshot.screen) must define a web DOM id")
        );

        let app_root_count = document
            .query_selector(&app_root_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let modal_region_count = document
            .query_selector(&modal_region_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let onboarding_root_count = document
            .query_selector(&onboarding_root_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let toast_region_count = document
            .query_selector(&toast_region_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);
        let active_screen_root_count = document
            .query_selector(&screen_selector)
            .ok()
            .flatten()
            .map(|_| 1)
            .unwrap_or(0);

        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("screen"),
            &JsValue::from_str(&format!("{:?}", snapshot.screen).to_ascii_lowercase()),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("app_root_count"),
            &JsValue::from_f64(f64::from(app_root_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("modal_region_count"),
            &JsValue::from_f64(f64::from(modal_region_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("onboarding_root_count"),
            &JsValue::from_f64(f64::from(onboarding_root_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("toast_region_count"),
            &JsValue::from_f64(f64::from(toast_region_count)),
        );
        let _ = Reflect::set(
            &payload,
            &JsValue::from_str("active_screen_root_count"),
            &JsValue::from_f64(f64::from(active_screen_root_count)),
        );
        payload.into()
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("root_structure"),
        root_structure.as_ref().unchecked_ref(),
    )?;
    root_structure.forget();

    let inject_message = Closure::wrap(Box::new(move |message: JsValue| -> JsValue {
        if let Some(text) = message.as_string() {
            if let Ok(controller) = current_controller() {
                controller.inject_message(&text);
            }
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("inject_message"),
        inject_message.as_ref().unchecked_ref(),
    )?;
    inject_message.forget();

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is not available"))?;
    install_page_owned_mutation_queues(&window)?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_HARNESS__"),
        &harness,
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_HARNESS_OBSERVE__"),
        &observe,
    )?;
    update_publication_state(
        &window,
        UI_PUBLICATION_STATE_KEY,
        "ui_state",
        "unavailable",
        "semantic_snapshot_not_published_yet",
        "observation_only",
    );
    update_publication_state(
        &window,
        RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
        "render_heartbeat",
        "unavailable",
        "render_heartbeat_not_published_yet",
        "observation_only",
    );
    let (semantic_submit_status, _) = semantic_submit_surface_state();
    refresh_semantic_submit_surface(&window, "semantic_bridge");
    web_sys::console::log_1(
        &format!(
            "[web-harness] semantic submit surface status={semantic_submit_status};generation={}",
            ACTIVE_GENERATION.with(|slot| slot.get())
        )
        .into(),
    );
    let read_only_ui_state = Closure::wrap(Box::new(move || -> JsValue {
        if current_controller().is_err() {
            return JsValue::NULL;
        }
        let Some(window) = web_sys::window() else {
            return JsValue::NULL;
        };
        published_ui_snapshot_value(&window)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE__"),
        read_only_ui_state.as_ref().unchecked_ref(),
    )?;
    read_only_ui_state.forget();

    let render_heartbeat = Closure::wrap(Box::new(move || -> JsValue {
        let window = match web_sys::window() {
            Some(window) => window,
            None => return JsValue::NULL,
        };
        Reflect::get(
            window.as_ref(),
            &JsValue::from_str("__AURA_RENDER_HEARTBEAT__"),
        )
        .unwrap_or(JsValue::NULL)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("render_heartbeat"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_STATE__"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    render_heartbeat.forget();

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn harness_bridge_selection_helpers_use_canonical_snapshot_selections_only() {
        let source = include_str!("harness_bridge.rs");

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
        assert!(
            channel_block.contains(".selected_channel_id()"),
            "selected_channel_id must use the controller-owned selected channel"
        );
        assert!(
            !channel_block.contains(".selected_item_id(ListId::Channels)"),
            "selected_channel_id command paths must not re-read observed channel selection"
        );

        let device_start = source
            .find("fn selected_device_id(controller: &UiController) -> Result<String, JsValue> {")
            .unwrap_or_else(|| panic!("missing selected_device_id"));
        let device_end = source[device_start..]
            .find("fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .map(|offset| device_start + offset)
            .unwrap_or(source.len());
        let device_block = &source[device_start..device_end];
        assert!(
            device_block.contains(".selected_item_id(ListId::Devices)"),
            "selected_device_id must use the canonical exported device selection"
        );
        assert!(
            !device_block.contains("list.items.len() == 1"),
            "selected_device_id must not infer selection from singleton device lists"
        );

        let authority_start = source
            .find("fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .unwrap_or_else(|| panic!("missing selected_authority_id"));
        let authority_end = source[authority_start..]
            .find("pub(crate) fn publish_semantic_controller_snapshot(controller: Arc<UiController>) -> UiSnapshot {")
            .map(|offset| authority_start + offset)
            .unwrap_or(source.len());
        let authority_block = &source[authority_start..authority_end];
        assert!(
            authority_block.contains(".selected_authority_id()"),
            "selected_authority_id must use the controller-owned selected authority"
        );
        assert!(
            !authority_block.contains(".selected_item_id(ListId::Authorities)"),
            "selected_authority_id command paths must not re-read observed authority selection"
        );

        assert!(
            source.contains("let stage_result = workflows::stage_account_creation(controller.app_core(), &account_name)"),
            "harness account creation should route through the shared aura-web workflow helper"
        );

        let import_start = source
            .find("IntentAction::ImportDeviceEnrollmentCode { code } => {")
            .unwrap_or_else(|| panic!("missing import device enrollment branch"));
        let import_end = source[import_start..]
            .find("IntentAction::OpenSettingsSection(section) => {")
            .map(|offset| import_start + offset)
            .unwrap_or(source.len());
        let import_block = &source[import_start..import_end];
        assert!(
            import_block.contains("workflows::accept_device_enrollment_import("),
            "harness device enrollment import should route through the shared aura-web workflow helper"
        );

        let removal_helper_start = source
            .find("async fn start_and_monitor_runtime_device_removal(")
            .unwrap_or_else(|| panic!("missing start_and_monitor_runtime_device_removal"));
        let removal_helper_end = source[removal_helper_start..]
            .find("#[derive(Clone, Debug)]")
            .map(|offset| removal_helper_start + offset)
            .unwrap_or(source.len());
        let removal_helper_block = &source[removal_helper_start..removal_helper_end];
        assert!(
            removal_helper_block.contains("monitor_key_rotation_ceremony_with_policy("),
            "web device removal must retain ceremony ownership long enough to drive the shared completion monitor"
        );
        assert!(
            removal_helper_block.contains("get_key_rotation_ceremony_status(&app_core, &status_handle).await"),
            "web device removal must check for immediate ceremony completion on the authoritative app path"
        );
        assert!(
            removal_helper_block.contains("settings_workflows::refresh_settings_from_runtime(&app_core)"),
            "web device removal must refresh authoritative settings when removal completes immediately"
        );
        assert!(
            removal_helper_block.contains("refresh_settings_on_complete: true"),
            "web device removal monitoring must preserve the shared authoritative settings refresh contract"
        );
        assert!(
            removal_helper_block.contains("shared_web_task_owner().spawn_local"),
            "web device removal monitoring must run on the sanctioned browser task owner"
        );

        let removal_start = source
            .find("IntentAction::RemoveSelectedDevice { device_id } => {")
            .unwrap_or_else(|| panic!("missing remove selected device branch"));
        let removal_end = source[removal_start..]
            .find("IntentAction::SwitchAuthority { authority_id } => {")
            .map(|offset| removal_start + offset)
            .unwrap_or(source.len());
        let removal_block = &source[removal_start..removal_end];
        assert!(
            removal_block.contains(
                "start_and_monitor_runtime_device_removal(controller.clone(), device_id).await?;"
            ),
            "web remove-selected-device must delegate to the shared start-and-monitor helper"
        );
        assert!(
            !removal_block
                .contains("start_device_removal_ceremony(&controller.app_core(), device_id)"),
            "web remove-selected-device must not drop the ceremony handle immediately after start"
        );

        let publish_start = source
            .find("pub(crate) fn publish_semantic_controller_snapshot(controller: Arc<UiController>) -> UiSnapshot {")
            .unwrap_or_else(|| panic!("missing publish_semantic_controller_snapshot"));
        let publish_end = source[publish_start..]
            .find("pub(crate) async fn schedule_browser_ui_mutation(")
            .map(|offset| publish_start + offset)
            .unwrap_or(source.len());
        let publish_block = &source[publish_start..publish_end];
        assert!(
            publish_block.contains("set_controller(controller.clone());"),
            "published semantic controller snapshots must refresh the harness bridge controller owner"
        );
        assert!(
            publish_block.contains("if snapshot_marks_generation_ready(snapshot) {\n        mark_generation_ready(generation_id);\n    }"),
            "render-aligned semantic publication must only mark the active generation ready after a semantically ready snapshot"
        );

        let generation_start = source
            .find("pub fn set_active_generation(generation_id: u64) {")
            .unwrap_or_else(|| panic!("missing set_active_generation"));
        let generation_end = source[generation_start..]
            .find("pub async fn wait_for_generation_ready(generation_id: u64)")
            .map(|offset| generation_start + offset)
            .unwrap_or(source.len());
        let generation_block = &source[generation_start..generation_end];
        assert!(
            generation_block.contains("ACTIVE_GENERATION"),
            "set_active_generation must update the active browser shell generation"
        );
        assert!(
            generation_block.contains("sync_generation_globals(&window);"),
            "set_active_generation must keep page-owned generation diagnostics synchronized"
        );
        assert!(
            source.contains("pub fn set_browser_shell_phase(phase: BrowserShellPhase)"),
            "browser shell generations must publish an explicit phase diagnostic"
        );
        assert!(
            source.contains("UI_GENERATION_PHASE_KEY"),
            "browser shell phase publication must use a page-owned diagnostics key"
        );

        let wait_start = source
            .find("pub async fn wait_for_generation_ready(generation_id: u64) -> Result<(), JsValue> {")
            .unwrap_or_else(|| panic!("missing wait_for_generation_ready"));
        let wait_end = source[wait_start..]
            .find("pub(crate) async fn schedule_browser_ui_mutation(")
            .map(|offset| wait_start + offset)
            .unwrap_or(source.len());
        let wait_block = &source[wait_start..wait_end];
        assert!(
            wait_block.contains("GENERATION_READY_WAITERS"),
            "wait_for_generation_ready must wait on explicit generation-ready ownership, not ambient polling"
        );
        assert!(
            source.contains("fn snapshot_marks_generation_ready(snapshot: &UiSnapshot) -> bool {")
                && source.contains("snapshot.readiness == UiReadiness::Ready"),
            "browser generation readiness must be derived from the shared UiReadiness contract"
        );

        let set_controller_start = source
            .find("pub fn set_controller(controller: Arc<UiController>) {")
            .unwrap_or_else(|| panic!("missing set_controller"));
        let set_controller_end = source[set_controller_start..]
            .find("fn current_controller() -> Result<Arc<UiController>, JsValue> {")
            .map(|offset| set_controller_start + offset)
            .unwrap_or(source.len());
        let set_controller_block = &source[set_controller_start..set_controller_end];
        assert!(
            set_controller_block.contains("Arc::ptr_eq"),
            "set_controller must only reset publication dedup when the controller owner changes"
        );

        let clear_controller_start = source
            .find("pub fn clear_controller(reason: &str) {")
            .unwrap_or_else(|| panic!("missing clear_controller"));
        let clear_controller_end = source[clear_controller_start..]
            .find("fn current_controller() -> Result<Arc<UiController>, JsValue> {")
            .map(|offset| clear_controller_start + offset)
            .unwrap_or(source.len());
        let clear_controller_block = &source[clear_controller_start..clear_controller_end];
        assert!(
            clear_controller_block.contains("*slot.borrow_mut() = None;"),
            "clear_controller must drop the active browser controller owner during generation rebinding"
        );
        assert!(
            clear_controller_block.contains("generation_rebinding"),
            "clear_controller must surface rebinding through explicit publication-state degradation"
        );
        assert!(
            clear_controller_block.contains("ACTIVE_GENERATION.with"),
            "clear_controller must drop the active generation during rebinding"
        );
        assert!(
            clear_controller_block.contains("publish_semantic_submit_state(&window, \"unavailable\", reason, \"generation_rebinding\")"),
            "clear_controller must degrade the semantic submit surface during generation rebinding"
        );

        let install_start = source
            .find("pub fn install_window_harness_api() -> Result<(), JsValue> {")
            .unwrap_or_else(|| panic!("missing install_window_harness_api"));
        let install_end = source[install_start..]
            .find("fn js_value_detail(value: &JsValue) -> String {")
            .map(|offset| install_start + offset)
            .unwrap_or(source.len());
        let install_block = &source[install_start..install_end];
        assert!(
            source.contains("fn publish_semantic_submit_state("),
            "browser harness installation must use an explicit semantic submit publication helper"
        );
        assert!(
            install_block.contains("publish_semantic_submit_state("),
            "browser harness installation must publish an explicit semantic submit readiness state"
        );
        assert!(
            install_block.contains("semantic_submit_surface_ready"),
            "browser harness installation must report a concrete semantic submit-ready detail"
        );
        assert!(
            install_block.contains("install_page_owned_mutation_queues(&window)?;")
                && source.contains("window.__AURA_DRIVER_SEMANTIC_ENQUEUE__ = (payloadJson) => {")
                && source.contains("submitState?.status === \"ready\"")
                && source.contains("window.__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__")
                && source.contains("window.__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__")
                && source.contains("&JsValue::from_str(\"enqueue_ready\")"),
            "browser harness bridge must own a generation-aware page semantic queue instead of delegating semantic replay ownership to the driver"
        );

        let mutation_start = source
            .find("pub(crate) async fn schedule_browser_ui_mutation(")
            .unwrap_or_else(|| panic!("missing schedule_browser_ui_mutation"));
        let mutation_end = source[mutation_start..]
            .find("async fn submit_semantic_command(")
            .map(|offset| mutation_start + offset)
            .unwrap_or(source.len());
        let mutation_block = &source[mutation_start..mutation_end];
        assert!(
            !mutation_block.contains("controller.finalize_account_setup(ScreenId::Neighborhood);"),
            "browser ui mutation scheduling must not repair readiness locally during generation execution"
        );
    }

    #[test]
    fn shared_browser_submission_branches_seed_exact_operation_handles() {
        let source = include_str!("harness_bridge.rs");

        assert!(
            source.contains("fn begin_exact_ui_operation("),
            "browser shared semantic flows must seed an exact UI operation handle before workflow handoff"
        );
        assert!(
            source.contains(
                "controller.begin_exact_operation_submission(captured_operation_id.clone())"
            ),
            "browser exact-operation helper must use the shared UiController submission path"
        );

        let create_contact_start = source
            .find("IntentAction::CreateContactInvitation {")
            .unwrap_or_else(|| panic!("missing create contact invitation branch"));
        let create_contact_end = source[create_contact_start..]
            .find("IntentAction::AcceptContactInvitation { code } => {")
            .map(|offset| create_contact_start + offset)
            .unwrap_or(source.len());
        let create_contact_block = &source[create_contact_start..create_contact_end];
        assert!(
            create_contact_block.contains(
                "begin_exact_ui_operation(controller.clone(), OperationId::invitation_create())"
            ),
            "browser contact invitation creation must allocate an exact invitation_create handle"
        );
        assert!(
            create_contact_block.contains("create_contact_invitation_with_instance(")
                && create_contact_block.contains("Some(handle.instance_id().clone())"),
            "browser contact invitation creation must hand off the exact instance id to the shared workflow"
        );
        assert!(
            create_contact_block.contains("semantic_response_with_handle("),
            "browser contact invitation creation must return the canonical handle to the harness"
        );

        let accept_pending_start = source
            .find("IntentAction::AcceptPendingChannelInvitation => {")
            .unwrap_or_else(|| panic!("missing accept pending channel invitation branch"));
        let accept_pending_end = source[accept_pending_start..]
            .find("IntentAction::JoinChannel { channel_name } => {")
            .map(|offset| accept_pending_start + offset)
            .unwrap_or(source.len());
        let accept_pending_block = &source[accept_pending_start..accept_pending_end];
        assert!(
            accept_pending_block.contains(
                "begin_exact_ui_operation(controller.clone(), OperationId::invitation_accept())"
            ),
            "browser pending channel acceptance must allocate an exact invitation_accept handle"
        );
        assert!(
            accept_pending_block.contains(
                "accept_pending_channel_invitation_with_binding_terminal_status("
            ) && accept_pending_block.contains("semantic_unit_result_with_handle(handle)"),
            "browser pending channel acceptance must hand off the exact instance and return an immediate handled acceptance"
        );

        let join_start = source
            .find("IntentAction::JoinChannel { channel_name } => {")
            .unwrap_or_else(|| panic!("missing join channel branch"));
        let join_end = source[join_start..]
            .find("IntentAction::InviteActorToChannel {")
            .map(|offset| join_start + offset)
            .unwrap_or(source.len());
        let join_block = &source[join_start..join_end];
        assert!(
            join_block.contains(
                "begin_exact_ui_operation(controller.clone(), OperationId::join_channel())"
            ),
            "browser join channel must allocate an exact join_channel handle"
        );
        assert!(
            join_block.contains("join_channel_by_name_with_instance(")
                && join_block.contains("semantic_channel_result_with_handle("),
            "browser join channel must hand off and return the exact handle"
        );

        let invite_start = source
            .find("IntentAction::InviteActorToChannel {")
            .unwrap_or_else(|| panic!("missing invite actor to channel branch"));
        let invite_end = source[invite_start..]
            .find("IntentAction::SendChatMessage { message } => {")
            .map(|offset| invite_start + offset)
            .unwrap_or(source.len());
        let invite_block = &source[invite_start..invite_end];
        assert!(
            invite_block.contains(
                "begin_exact_ui_operation(controller.clone(), OperationId::invitation_create())"
            ),
            "browser invite-to-channel must allocate an exact invitation_create handle"
        );
        assert!(
            invite_block.contains("invite_user_to_channel_with_context(")
                && invite_block.contains("semantic_unit_result_with_handle(handle)"),
            "browser invite-to-channel must return the exact handle after workflow handoff"
        );

        let send_start = source
            .find("IntentAction::SendChatMessage { message } => {")
            .unwrap_or_else(|| panic!("missing send chat message branch"));
        let send_end = source[send_start..]
            .find("\n    }\n}\n\nfn publish_ui_snapshot_now(")
            .map(|offset| send_start + offset)
            .unwrap_or(source.len());
        let send_block = &source[send_start..send_end];
        assert!(
            send_block.contains(
                "begin_exact_ui_operation(controller.clone(), OperationId::send_message())"
            ),
            "browser send-message must allocate an exact send_message handle"
        );
        assert!(
            send_block.contains("send_message_with_instance(")
                && send_block.contains("semantic_unit_result_with_handle(handle)"),
            "browser send-message must hand off the exact instance and return the handle"
        );
    }

    #[test]
    fn semantic_ui_snapshot_publication_precedes_render_heartbeat_scheduling() {
        let source = include_str!("harness_bridge.rs");
        let publish_start = source
            .find("pub fn publish_ui_snapshot(snapshot: &UiSnapshot) {")
            .unwrap_or_else(|| panic!("missing publish_ui_snapshot"));
        let publish_end = source[publish_start..]
            .find("fn published_ui_snapshot_value(window: &web_sys::Window) -> JsValue {")
            .map(|offset| publish_start + offset)
            .unwrap_or(source.len());
        let publish_block = &source[publish_start..publish_end];

        let publish_now_index = publish_block
            .find("if !publish_ui_snapshot_now(")
            .unwrap_or_else(|| panic!("publish_ui_snapshot must publish immediately"));
        let raf_index = publish_block
            .find("window.request_animation_frame(callback_fn)")
            .unwrap_or_else(|| panic!("publish_ui_snapshot must still schedule render heartbeat"));
        assert!(
            publish_now_index < raf_index,
            "semantic snapshot publication must happen before RAF heartbeat scheduling"
        );
        assert!(
            publish_block.contains("snapshot.revision.render_seq = Some(render_seq);"),
            "browser semantic snapshot publication must attach render-sequence freshness metadata"
        );
        assert!(
            publish_block.contains("RENDER_HEARTBEAT_PUBLICATION_STATE_KEY"),
            "RAF failures must degrade the render heartbeat publication state, not ui_state"
        );
    }
}

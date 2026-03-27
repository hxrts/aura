//! JavaScript harness API bridge for browser-based testing.
//!
//! Exposes the UiController to JavaScript via window.harness, enabling the test
//! harness to send keys, capture screenshots, and query UI state from Playwright.

use async_lock::RwLock;
use aura_agent::AuraAgent;
use aura_app::AppCore;
use aura_core::{AuthorityId, DeviceId};
use aura_ui::UiController;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::JsValue;

use crate::harness::{
    generation::{
        self, browser_shell_phase_label, reset_published_ui_snapshot_dedup,
        set_active_generation_local, set_browser_shell_phase_local, BrowserShellPhase,
        UI_GENERATION_PHASE_KEY,
    },
    install, mutation,
    publication::{self, PublicationBindingMode},
};

thread_local! {
    static BOOTSTRAP_HANDOFF_SUBMITTER: RefCell<Option<Arc<dyn Fn(BootstrapHandoff) -> js_sys::Promise>>> = const { RefCell::new(None) };
    static RUNTIME_IDENTITY_STAGER: RefCell<Option<Arc<dyn Fn(String) -> js_sys::Promise>>> = const { RefCell::new(None) };
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

pub fn set_browser_shell_phase(phase: BrowserShellPhase) {
    set_browser_shell_phase_local(phase);
    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = js_sys::Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_GENERATION_PHASE_KEY),
        &JsValue::from_str(browser_shell_phase_label(phase)),
    );
    publication::refresh_semantic_submit_surface(&window, PublicationBindingMode::SemanticBridge);
}

pub fn set_active_generation(generation_id: u64) {
    set_active_generation_local(generation_id);
    if let Some(window) = web_sys::window() {
        generation::sync_generation_globals(&window);
        publication::refresh_semantic_submit_surface(
            &window,
            PublicationBindingMode::SemanticBridge,
        );
    }
}

pub async fn wait_for_generation_ready(generation_id: u64) -> Result<(), JsValue> {
    generation::wait_for_generation_ready(generation_id).await
}

pub(crate) fn schedule_browser_task_next_tick(
    action: impl FnOnce() + 'static,
) -> Result<(), JsValue> {
    mutation::schedule_browser_task_next_tick(action)
}

pub(crate) async fn schedule_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController) + 'static,
) -> Result<(), JsValue> {
    mutation::schedule_browser_ui_mutation(controller, action).await
}

pub(crate) fn apply_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController),
) {
    mutation::apply_browser_ui_mutation(controller, action);
}

pub(crate) fn publish_semantic_controller_snapshot(
    controller: Arc<UiController>,
) -> aura_app::ui::contract::UiSnapshot {
    publication::publish_semantic_controller_snapshot(controller)
}

pub fn set_controller(controller: Arc<UiController>) {
    aura_app::frontend_primitives::set_frontend_debug_probe(Some(Arc::new({
        let controller = controller.clone();
        move |message| {
            controller.info_toast(message.clone());
            controller.push_log(&message);
        }
    })));
    let controller_changed = generation::set_controller(controller);
    if controller_changed {
        reset_published_ui_snapshot_dedup();
    }
    if let Some(window) = web_sys::window() {
        publication::refresh_semantic_submit_surface(
            &window,
            PublicationBindingMode::SemanticBridge,
        );
    }
}

pub fn clear_controller(reason: &str) {
    aura_app::frontend_primitives::set_frontend_debug_probe(None);
    generation::clear_controller_state();
    set_browser_shell_phase(BrowserShellPhase::Rebinding);
    if let Some(window) = web_sys::window() {
        let window_ref = window.as_ref();
        let _ = js_sys::Reflect::set(
            window_ref,
            &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
            &JsValue::NULL,
        );
        let _ = js_sys::Reflect::set(
            window_ref,
            &JsValue::from_str("__AURA_UI_STATE_JSON__"),
            &JsValue::NULL,
        );
        generation::sync_generation_globals(&window);
        publication::mark_generation_rebinding(&window, reason);
    }
}

pub fn publish_ui_snapshot(snapshot: &aura_app::ui::contract::UiSnapshot) {
    publication::publish_ui_snapshot(snapshot);
}

pub fn install_window_harness_api(
    harness_transport_context: Option<(Arc<RwLock<AppCore>>, Arc<AuraAgent>)>,
) -> Result<(), JsValue> {
    install::install_window_harness_api(harness_transport_context)
}

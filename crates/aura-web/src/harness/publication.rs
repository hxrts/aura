use aura_app::ui::contract::{ModalId, RenderHeartbeat, ScreenId, UiSnapshot};
use aura_ui::UiController;
use js_sys::{Object, JSON};
use serde_wasm_bindgen::to_value;
use std::borrow::Cow;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::harness::browser_contract::{
    DRIVER_PUSH_RENDER_HEARTBEAT_KEY, DRIVER_PUSH_UI_STATE_KEY, RENDER_HEARTBEAT_JSON_KEY,
    RENDER_HEARTBEAT_KEY, RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
    SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY, UI_PUBLICATION_STATE_KEY, UI_STATE_CACHE_KEY,
    UI_STATE_JSON_KEY,
};
use crate::harness::driver_contract::{PUSH_SEMANTIC_SUBMIT_STATE_KEY, WAKE_SEMANTIC_QUEUE_KEY};
use crate::harness::generation::{
    active_generation, browser_shell_phase_label, current_bootstrap_transition_detail,
    current_browser_shell_phase, generation_js_value, mark_generation_ready, next_render_seq,
    note_published_ui_snapshot_json, ready_generation, BrowserShellPhase, UI_GENERATION_PHASE_KEY,
};
use crate::harness::mutation::schedule_browser_task_next_tick;
use crate::harness::page_owned_queue::wake_pending_mutation_queues_if_needed;
use crate::harness::window_contract::{object_set, HarnessWindowContract};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicationReliability {
    RequiredForCorrectness,
    RequiredForConvergence,
    Informational,
}

impl PublicationReliability {
    const fn label(self) -> &'static str {
        match self {
            Self::RequiredForCorrectness => "required_for_correctness",
            Self::RequiredForConvergence => "required_for_convergence",
            Self::Informational => "informational",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicationStatus {
    Ready,
    Published,
    Degraded,
    Unavailable,
}

impl PublicationStatus {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Published => "published",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicationBindingMode {
    DriverPush,
    WindowCacheOnly,
    ObservationOnly,
    SemanticBridge,
    GenerationRebinding,
}

impl PublicationBindingMode {
    const fn label(self) -> &'static str {
        match self {
            Self::DriverPush => "driver_push",
            Self::WindowCacheOnly => "window_cache_only",
            Self::ObservationOnly => "observation_only",
            Self::SemanticBridge => "semantic_bridge",
            Self::GenerationRebinding => "generation_rebinding",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PublicationState {
    status: PublicationStatus,
    detail: Cow<'static, str>,
    binding_mode: PublicationBindingMode,
}

impl PublicationState {
    fn new(
        status: PublicationStatus,
        detail: impl Into<Cow<'static, str>>,
        binding_mode: PublicationBindingMode,
    ) -> Self {
        Self {
            status,
            detail: detail.into(),
            binding_mode,
        }
    }

    fn detail(&self) -> &str {
        self.detail.as_ref()
    }

    pub(crate) fn status(&self) -> PublicationStatus {
        self.status
    }
}

#[derive(Clone, Copy, Debug)]
struct PublicationSurface {
    key: &'static str,
    name: &'static str,
    reliability: PublicationReliability,
}

const UI_SURFACE: PublicationSurface = PublicationSurface {
    key: UI_PUBLICATION_STATE_KEY,
    name: "ui_state",
    reliability: PublicationReliability::RequiredForCorrectness,
};

const RENDER_HEARTBEAT_SURFACE: PublicationSurface = PublicationSurface {
    key: RENDER_HEARTBEAT_PUBLICATION_STATE_KEY,
    name: "render_heartbeat",
    reliability: PublicationReliability::RequiredForConvergence,
};

const SEMANTIC_SUBMIT_SURFACE: PublicationSurface = PublicationSurface {
    key: SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY,
    name: "semantic_submit",
    reliability: PublicationReliability::Informational,
};

pub(crate) fn js_value_detail(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            JSON::stringify(error)
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

pub(crate) fn log_js_callback_error(context: &str, error: &JsValue) {
    let detail = js_value_detail(error);
    web_sys::console::error_1(&JsValue::from_str(&format!(
        "[web-harness] {context} failed: {detail}"
    )));
}

fn update_publication_state(
    window: &web_sys::Window,
    surface: PublicationSurface,
    state: &PublicationState,
) {
    let window_contract = HarnessWindowContract::new(window.clone());
    let state_object = Object::new();
    let _ = object_set(&state_object, "surface", &JsValue::from_str(surface.name));
    let _ = object_set(
        &state_object,
        "status",
        &JsValue::from_str(state.status.label()),
    );
    let _ = object_set(&state_object, "detail", &JsValue::from_str(state.detail()));
    let _ = object_set(
        &state_object,
        "binding_mode",
        &JsValue::from_str(state.binding_mode.label()),
    );
    let _ = object_set(
        &state_object,
        "reliability",
        &JsValue::from_str(surface.reliability.label()),
    );
    if let Err(error) = window_contract.set(surface.key, state_object.as_ref()) {
        web_sys::console::error_1(&JsValue::from_str(&format!(
            "[web-harness] failed to update publication state {}: {}",
            surface.key,
            js_value_detail(&error)
        )));
    }
}

fn semantic_submit_surface_detail(base: &'static str) -> Cow<'static, str> {
    if let Some(detail) = current_bootstrap_transition_detail() {
        Cow::Owned(format!("{base}:{detail}"))
    } else {
        Cow::Borrowed(base)
    }
}

pub(crate) fn schedule_window_callback_push(
    window: &web_sys::Window,
    function_key: &str,
    payload: JsValue,
    callback_label: &'static str,
) -> bool {
    let window_contract = HarnessWindowContract::new(window.clone());
    if window_contract.function(function_key).is_none() {
        return false;
    }

    let callback_window = window.clone();
    let callback_key = function_key.to_string();
    let schedule_result = schedule_browser_task_next_tick(move || {
        let callback_contract = HarnessWindowContract::new(callback_window.clone());
        let Some(function) = callback_contract.function(&callback_key) else {
            return;
        };
        if let Err(error) = function.call1(callback_window.as_ref(), &payload) {
            log_js_callback_error(callback_label, &error);
        }
    });
    if let Err(error) = schedule_result {
        log_js_callback_error(callback_label, &error);
        return false;
    }
    true
}

fn publish_ui_snapshot_now(
    window: &web_sys::Window,
    value: JsValue,
    json: String,
    screen: ScreenId,
    modal: Option<ModalId>,
    operation_count: usize,
) -> bool {
    let window_contract = HarnessWindowContract::new(window.clone());
    let cache_publish = window_contract.set(UI_STATE_CACHE_KEY, &value);
    let json_publish = window_contract.set(UI_STATE_JSON_KEY, &JsValue::from_str(&json));

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

    let driver_push_published = schedule_window_callback_push(
        window,
        DRIVER_PUSH_UI_STATE_KEY,
        JsValue::from_str(&json),
        "driver UI state push",
    );
    let binding_mode = if driver_push_published {
        PublicationBindingMode::DriverPush
    } else {
        PublicationBindingMode::WindowCacheOnly
    };
    if binding_mode == PublicationBindingMode::WindowCacheOnly {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-publish]binding={};screen={screen:?};modal={modal:?};ops={operation_count}",
            binding_mode.label(),
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-state]screen={screen:?};modal={modal:?};ops={operation_count};binding={}",
            binding_mode.label(),
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!("[aura-ui-json]{json}")));
    }

    let has_observable_publication = cache_published || json_published || driver_push_published;
    let state = if !has_observable_publication {
        PublicationState::new(
            PublicationStatus::Unavailable,
            publication_issues.join(" | "),
            binding_mode,
        )
    } else if publication_issues.is_empty() {
        PublicationState::new(PublicationStatus::Published, "published", binding_mode)
    } else {
        PublicationState::new(
            PublicationStatus::Degraded,
            publication_issues.join(" | "),
            binding_mode,
        )
    };
    update_publication_state(window, UI_SURFACE, &state);
    web_sys::console::log_1(
        &format!(
            "[web-harness-publish] ui_state status={};binding={};screen={screen:?};modal={modal:?};ops={operation_count}",
            state.status.label(),
            state.binding_mode.label(),
        )
        .into(),
    );

    has_observable_publication
}

pub(crate) fn publish_render_heartbeat(window: &web_sys::Window, heartbeat: &RenderHeartbeat) {
    let Ok(value) = to_value(heartbeat) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };

    let window_contract = HarnessWindowContract::new(window.clone());
    let heartbeat_publish = window_contract.set(RENDER_HEARTBEAT_KEY, &value);
    let heartbeat_json_publish =
        window_contract.set(RENDER_HEARTBEAT_JSON_KEY, &JsValue::from_str(&json));

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

    let driver_push_published = schedule_window_callback_push(
        window,
        DRIVER_PUSH_RENDER_HEARTBEAT_KEY,
        JsValue::from_str(&json),
        "driver render heartbeat push",
    );

    let binding_mode = if driver_push_published {
        PublicationBindingMode::DriverPush
    } else {
        PublicationBindingMode::WindowCacheOnly
    };
    let has_observable_publication =
        heartbeat_published || heartbeat_json_published || driver_push_published;
    let state = if !has_observable_publication {
        PublicationState::new(
            PublicationStatus::Unavailable,
            publication_issues.join(" | "),
            binding_mode,
        )
    } else if publication_issues.is_empty() {
        PublicationState::new(PublicationStatus::Published, "published", binding_mode)
    } else {
        PublicationState::new(
            PublicationStatus::Degraded,
            publication_issues.join(" | "),
            binding_mode,
        )
    };
    update_publication_state(window, RENDER_HEARTBEAT_SURFACE, &state);
}

pub(crate) fn semantic_submit_surface_state() -> PublicationState {
    let active_generation = active_generation();
    let has_controller = crate::harness::generation::current_controller().is_some();
    let phase = current_browser_shell_phase();
    let queue_installed = web_sys::window()
        .map(HarnessWindowContract::new)
        .and_then(|window_contract| window_contract.function(WAKE_SEMANTIC_QUEUE_KEY))
        .is_some();
    if active_generation == 0 {
        return PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_missing_generation"),
            PublicationBindingMode::SemanticBridge,
        );
    }
    if !has_controller {
        return PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_missing_controller"),
            PublicationBindingMode::SemanticBridge,
        );
    }
    if !queue_installed {
        return PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_missing_queue"),
            PublicationBindingMode::SemanticBridge,
        );
    }
    match phase {
        BrowserShellPhase::Ready => PublicationState::new(
            PublicationStatus::Ready,
            semantic_submit_surface_detail("semantic_submit_surface_ready"),
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::Bootstrapping => PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_bootstrapping"),
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::HandoffCommitted => PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_handoff_committed"),
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::Rebinding => PublicationState::new(
            PublicationStatus::Unavailable,
            semantic_submit_surface_detail("semantic_submit_surface_generation_rebinding"),
            PublicationBindingMode::GenerationRebinding,
        ),
    }
}

pub(crate) fn publish_semantic_submit_state(window: &web_sys::Window, state: &PublicationState) {
    let window_contract = HarnessWindowContract::new(window.clone());
    update_publication_state(window, SEMANTIC_SUBMIT_SURFACE, state);

    let Ok(state_object) = window_contract.get(SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY) else {
        return;
    };
    if state_object.is_null() || state_object.is_undefined() {
        return;
    }

    let generation_id = active_generation();
    let ready_generation = ready_generation();
    let controller_present = crate::harness::generation::current_controller().is_some();
    let phase = window_contract
        .get(UI_GENERATION_PHASE_KEY)
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| browser_shell_phase_label(BrowserShellPhase::Rebinding).to_string());
    let Some(state_object) = state_object.dyn_into::<Object>().ok() else {
        return;
    };
    let _ = object_set(
        &state_object,
        "generation_id",
        &generation_js_value(generation_id),
    );
    let _ = object_set(
        &state_object,
        "active_generation",
        &generation_js_value(generation_id),
    );
    let _ = object_set(
        &state_object,
        "ready_generation",
        &generation_js_value(ready_generation),
    );
    let _ = object_set(
        &state_object,
        "generation_ready",
        &JsValue::from_bool(generation_id > 0 && ready_generation >= generation_id),
    );
    let _ = object_set(&state_object, "phase", &JsValue::from_str(&phase));
    let _ = object_set(
        &state_object,
        "controller_present",
        &JsValue::from_bool(controller_present),
    );
    match current_bootstrap_transition_detail() {
        Some(detail) => {
            let _ = object_set(
                &state_object,
                "bootstrap_transition_detail",
                &JsValue::from_str(&detail),
            );
        }
        None => {
            let _ = object_set(&state_object, "bootstrap_transition_detail", &JsValue::NULL);
        }
    }
    let enqueue_ready = window_contract.function(WAKE_SEMANTIC_QUEUE_KEY).is_some();
    let _ = object_set(
        &state_object,
        "enqueue_ready",
        &JsValue::from_bool(enqueue_ready),
    );
    let _ = schedule_window_callback_push(
        window,
        PUSH_SEMANTIC_SUBMIT_STATE_KEY,
        JsValue::from(state_object),
        "driver semantic submit state push",
    );
    wake_pending_mutation_queues_if_needed(window);
}

pub(crate) fn refresh_semantic_submit_surface(
    window: &web_sys::Window,
    binding_mode: PublicationBindingMode,
) {
    let mut state = semantic_submit_surface_state();
    state.binding_mode = binding_mode;
    web_sys::console::log_1(
        &format!(
            "[web-harness] semantic_submit_refresh status={};detail={};generation={};phase={}",
            state.status.label(),
            state.detail(),
            active_generation(),
            browser_shell_phase_label(current_browser_shell_phase())
        )
        .into(),
    );
    publish_semantic_submit_state(window, &state);
}

pub(crate) fn initialize_harness_publication_state(window: &web_sys::Window) {
    update_publication_state(
        window,
        UI_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_snapshot_not_published_yet",
            PublicationBindingMode::ObservationOnly,
        ),
    );
    update_publication_state(
        window,
        RENDER_HEARTBEAT_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            "render_heartbeat_not_published_yet",
            PublicationBindingMode::ObservationOnly,
        ),
    );
}

pub(crate) fn mark_generation_rebinding(window: &web_sys::Window, reason: &str) {
    update_publication_state(
        window,
        UI_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            reason.to_string(),
            PublicationBindingMode::GenerationRebinding,
        ),
    );
    update_publication_state(
        window,
        RENDER_HEARTBEAT_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            reason.to_string(),
            PublicationBindingMode::GenerationRebinding,
        ),
    );
    publish_semantic_submit_state(
        window,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            reason.to_string(),
            PublicationBindingMode::GenerationRebinding,
        ),
    );
}

pub(crate) fn published_ui_snapshot_value(window: &web_sys::Window) -> JsValue {
    let window_contract = HarnessWindowContract::new(window.clone());
    let published_json = window_contract
        .get(UI_STATE_JSON_KEY)
        .ok()
        .filter(|value| !value.is_null() && !value.is_undefined());

    if let Some(published_json) = published_json {
        if published_json.as_string().is_some() {
            return published_json;
        }
    }

    let published_cache = window_contract
        .get(UI_STATE_CACHE_KEY)
        .ok()
        .filter(|value| !value.is_null() && !value.is_undefined());
    if let Some(published_cache) = published_cache {
        return published_cache;
    }

    update_publication_state(
        window,
        UI_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_snapshot_not_published",
            PublicationBindingMode::ObservationOnly,
        ),
    );
    JsValue::NULL
}

pub(crate) fn publish_ui_snapshot(snapshot: &UiSnapshot) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let mut dedup_snapshot = snapshot.clone();
    dedup_snapshot.revision.render_seq = None;
    let Ok(dedup_value) = to_value(&dedup_snapshot) else {
        return;
    };
    let Ok(dedup_json) = JSON::stringify(&dedup_value) else {
        return;
    };
    let Some(dedup_json) = dedup_json.as_string() else {
        return;
    };
    if !note_published_ui_snapshot_json(&dedup_json) {
        return;
    }

    let render_seq = next_render_seq();
    let mut published_snapshot = snapshot.clone();
    published_snapshot.revision.render_seq = Some(render_seq);
    let Ok(value) = to_value(&published_snapshot) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };
    let screen = published_snapshot.screen;
    let open_modal = published_snapshot.open_modal;
    let operation_count = published_snapshot.operations.len();
    if !publish_ui_snapshot_now(&window, value, json, screen, open_modal, operation_count) {
        return;
    }
    let generation_id = active_generation();
    // Generation readiness tracks the canonical publication boundary for the
    // active browser shell generation, not whether the domain is already at
    // `UiReadiness::Ready`. Shared semantic startup/restart recovery must be
    // allowed to proceed once the current generation has published its first
    // authoritative snapshot, including pre-account and other non-ready
    // screens.
    mark_generation_ready(generation_id);

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
            RENDER_HEARTBEAT_SURFACE,
            &PublicationState::new(
                PublicationStatus::Unavailable,
                format!(
                    "request_animation_frame_failed: {}",
                    js_value_detail(&error)
                ),
                PublicationBindingMode::DriverPush,
            ),
        );
    }
}

pub(crate) fn publish_semantic_controller_snapshot(controller: Arc<UiController>) -> UiSnapshot {
    crate::harness_bridge::set_controller(controller.clone());
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

#[cfg(test)]
mod tests {
    #[test]
    fn ui_snapshot_dedup_happens_before_render_seq_is_added() {
        let source = include_str!("publication.rs");
        let start = source
            .find("pub(crate) fn publish_ui_snapshot(snapshot: &UiSnapshot) {")
            .expect("publish_ui_snapshot definition");
        let end = source[start..]
            .find("pub(crate) fn publish_semantic_controller_snapshot")
            .map(|offset| start + offset)
            .expect("publish_semantic_controller_snapshot marker");
        let body = &source[start..end];
        let dedup_index = body
            .find("note_published_ui_snapshot_json(&dedup_json)")
            .expect("dedup marker");
        let render_seq_index = body
            .find("let render_seq = next_render_seq();")
            .expect("render_seq marker");
        assert!(
            dedup_index < render_seq_index,
            "stable UI snapshot dedup must happen before render_seq is assigned"
        );
        assert!(
            body.contains("dedup_snapshot.revision.render_seq = None;"),
            "dedup must ignore render heartbeat sequencing"
        );
    }

    #[test]
    fn semantic_submit_publication_exposes_generation_owned_bootstrap_fields() {
        let source = include_str!("publication.rs");
        let start = source
            .find("pub(crate) fn publish_semantic_submit_state(window: &web_sys::Window, state: &PublicationState) {")
            .expect("publish_semantic_submit_state definition");
        let end = source[start..]
            .find("pub(crate) fn refresh_semantic_submit_surface(")
            .map(|offset| start + offset)
            .expect("refresh_semantic_submit_surface marker");
        let body = &source[start..end];
        for required in [
            "\"active_generation\"",
            "\"ready_generation\"",
            "\"generation_ready\"",
            "\"controller_present\"",
            "\"bootstrap_transition_detail\"",
        ] {
            assert!(
                body.contains(required),
                "semantic submit publication should expose {required} for observed bootstrap ownership"
            );
        }
    }
}

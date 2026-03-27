use aura_app::ui::contract::{ModalId, RenderHeartbeat, ScreenId, UiReadiness, UiSnapshot};
use aura_ui::UiController;
use js_sys::{Function, Object, Reflect, JSON};
use serde_wasm_bindgen::to_value;
use std::borrow::Cow;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::harness::generation::{
    active_generation, browser_shell_phase_label, current_browser_shell_phase, generation_js_value,
    mark_generation_ready, next_render_seq, note_published_ui_snapshot_json, BrowserShellPhase,
    UI_GENERATION_PHASE_KEY,
};

pub(crate) const UI_PUBLICATION_STATE_KEY: &str = "__AURA_UI_PUBLICATION_STATE__";
pub(crate) const RENDER_HEARTBEAT_PUBLICATION_STATE_KEY: &str =
    "__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__";
pub(crate) const SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY: &str =
    "__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__";

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
    let state_object = Object::new();
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("surface"),
        &JsValue::from_str(surface.name),
    );
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("status"),
        &JsValue::from_str(state.status.label()),
    );
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("detail"),
        &JsValue::from_str(state.detail()),
    );
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("binding_mode"),
        &JsValue::from_str(state.binding_mode.label()),
    );
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("reliability"),
        &JsValue::from_str(surface.reliability.label()),
    );
    if let Err(error) = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(surface.key),
        state_object.as_ref(),
    ) {
        web_sys::console::error_1(&JsValue::from_str(&format!(
            "[web-harness] failed to update publication state {}: {}",
            surface.key,
            js_value_detail(&error)
        )));
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
            (PublicationBindingMode::DriverPush, false)
        } else {
            (PublicationBindingMode::DriverPush, true)
        }
    })
    .unwrap_or((PublicationBindingMode::WindowCacheOnly, false));
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

pub(crate) fn wake_page_owned_mutation_queues(window: &web_sys::Window) {
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

pub(crate) fn semantic_submit_surface_state() -> PublicationState {
    let active_generation = active_generation();
    let has_controller = crate::harness::generation::current_controller().is_some();
    let phase = current_browser_shell_phase();
    if active_generation == 0 {
        return PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_submit_surface_missing_generation",
            PublicationBindingMode::SemanticBridge,
        );
    }
    if !has_controller {
        return PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_submit_surface_missing_controller",
            PublicationBindingMode::SemanticBridge,
        );
    }
    match phase {
        BrowserShellPhase::Ready => PublicationState::new(
            PublicationStatus::Ready,
            "semantic_submit_surface_ready",
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::Bootstrapping => PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_submit_surface_bootstrapping",
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::HandoffCommitted => PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_submit_surface_handoff_committed",
            PublicationBindingMode::SemanticBridge,
        ),
        BrowserShellPhase::Rebinding => PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_submit_surface_generation_rebinding",
            PublicationBindingMode::GenerationRebinding,
        ),
    }
}

pub(crate) fn publish_semantic_submit_state(window: &web_sys::Window, state: &PublicationState) {
    update_publication_state(window, SEMANTIC_SUBMIT_SURFACE, state);

    let Ok(state_object) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY),
    ) else {
        return;
    };
    if state_object.is_null() || state_object.is_undefined() {
        return;
    }

    let generation_id = active_generation();
    let phase = Reflect::get(window.as_ref(), &JsValue::from_str(UI_GENERATION_PHASE_KEY))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| browser_shell_phase_label(BrowserShellPhase::Rebinding).to_string());
    let _ = Reflect::set(
        &state_object,
        &JsValue::from_str("generation_id"),
        &generation_js_value(generation_id),
    );
    let _ = Reflect::set(
        &state_object,
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
        &state_object,
        &JsValue::from_str("enqueue_ready"),
        &JsValue::from_bool(enqueue_ready),
    );
    if let Ok(function) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE"),
    )
    .and_then(|candidate| candidate.dyn_into::<Function>())
    {
        if let Err(error) = function.call1(window.as_ref(), &state_object) {
            log_js_callback_error("driver semantic submit state push", &error);
        }
    }
    wake_page_owned_mutation_queues(window);
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
        UI_SURFACE,
        &PublicationState::new(
            PublicationStatus::Unavailable,
            "semantic_snapshot_not_published",
            PublicationBindingMode::ObservationOnly,
        ),
    );
    JsValue::NULL
}

fn snapshot_marks_generation_ready(snapshot: &UiSnapshot) -> bool {
    snapshot.readiness == UiReadiness::Ready
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
    if snapshot_marks_generation_ready(&published_snapshot) {
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
}

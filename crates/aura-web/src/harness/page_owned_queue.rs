use aura_app::ui::contract::classify_screen_item_id;
use aura_app::ui::types::BootstrapRuntimeIdentity;
use js_sys::{Array, Function, JSON, Object, Reflect};
use serde::Deserialize;
use serde_json::from_str;
use std::cell::RefCell;
use std::collections::VecDeque;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::harness::commands;
use crate::harness::driver_contract::{
    MUTATION_QUEUE_INSTALLED_KEY, PENDING_NAV_SCREEN_KEY, PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY,
    PENDING_SEMANTIC_QUEUE_SEED_KEY, PUSH_RUNTIME_STAGE_RESULT_KEY,
    PUSH_SEMANTIC_RESULT_KEY, PUSH_SEMANTIC_SUBMIT_STATE_KEY, RUNTIME_STAGE_BUSY_KEY,
    RUNTIME_STAGE_DEBUG_KEY, RUNTIME_STAGE_ENQUEUE_KEY, RUNTIME_STAGE_QUEUE_KEY,
    RUNTIME_STAGE_RESULTS_KEY, RUNTIME_STAGE_WAKE_SCHEDULED_KEY, RuntimeStageQueuePayload,
    SEMANTIC_BUSY_KEY, SEMANTIC_DEBUG_KEY, SEMANTIC_ENQUEUE_KEY, SEMANTIC_QUEUE_KEY,
    SEMANTIC_RESULTS_KEY, SEMANTIC_WAKE_SCHEDULED_KEY, SemanticQueuePayload,
    WAKE_PENDING_NAV_KEY, WAKE_RUNTIME_STAGE_QUEUE_KEY, WAKE_SEMANTIC_QUEUE_KEY,
};
use crate::harness::generation::current_controller;
use crate::harness::mutation::{apply_browser_ui_mutation, schedule_browser_task_next_tick};
use crate::harness::publication::{
    semantic_submit_surface_state, PublicationStatus, SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY,
};
use crate::task_owner::shared_web_task_owner;

#[derive(Default)]
struct SemanticQueueState {
    queue: VecDeque<SemanticQueuePayload>,
    busy: bool,
    wake_scheduled: bool,
}

#[derive(Default)]
struct RuntimeStageQueueState {
    queue: VecDeque<RuntimeStageQueuePayload>,
    busy: bool,
    wake_scheduled: bool,
}

#[derive(Default)]
struct PendingNavState {
    wake_scheduled: bool,
}

thread_local! {
    static SEMANTIC_QUEUE_STATE: RefCell<SemanticQueueState> = RefCell::new(SemanticQueueState::default());
    static RUNTIME_STAGE_QUEUE_STATE: RefCell<RuntimeStageQueueState> = RefCell::new(RuntimeStageQueueState::default());
    static PENDING_NAV_STATE: RefCell<PendingNavState> = RefCell::new(PendingNavState::default());
}

pub(crate) fn install(window: &web_sys::Window) -> Result<(), JsValue> {
    web_sys::console::log_1(&"[web-harness-queue] install start".into());
    if Reflect::get(window.as_ref(), &JsValue::from_str(MUTATION_QUEUE_INSTALLED_KEY))
        .ok()
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        drain_seed_queues(window)?;
        web_sys::console::log_1(&"[web-harness-queue] install already-present".into());
        push_current_semantic_submit_state(window);
        wake_semantic_queue(0);
        wake_runtime_stage_queue(0);
        wake_pending_nav();
        return Ok(());
    }

    ensure_window_globals(window)?;
    install_semantic_enqueue(window)?;
    install_runtime_stage_enqueue(window)?;
    install_wake_semantic_queue(window)?;
    install_wake_runtime_stage_queue(window)?;
    install_pending_nav_wake(window)?;
    drain_seed_queues(window)?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(MUTATION_QUEUE_INSTALLED_KEY),
        &JsValue::TRUE,
    )?;
    web_sys::console::log_1(&"[web-harness-queue] install complete".into());
    push_current_semantic_submit_state(window);
    wake_semantic_queue(0);
    wake_runtime_stage_queue(0);
    wake_pending_nav();
    Ok(())
}

fn drain_seed_queues(window: &web_sys::Window) -> Result<(), JsValue> {
    let semantic_seeded =
        seed_queue_from_window(window, PENDING_SEMANTIC_QUEUE_SEED_KEY, |payload| {
            SEMANTIC_QUEUE_STATE.with(|state| state.borrow_mut().queue.push_back(payload));
        })?;
    if semantic_seeded > 0 {
        web_sys::console::log_1(
            &format!("[web-harness-queue] semantic seed accepted count={semantic_seeded}").into(),
        );
    }
    let runtime_stage_seeded =
        seed_queue_from_window(window, PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY, |payload| {
            RUNTIME_STAGE_QUEUE_STATE.with(|state| state.borrow_mut().queue.push_back(payload));
        })?;
    if runtime_stage_seeded > 0 {
        web_sys::console::log_1(
            &format!(
                "[web-harness-queue] runtime-stage seed accepted count={runtime_stage_seeded}"
            )
            .into(),
        );
    }
    Ok(())
}

fn ensure_window_globals(window: &web_sys::Window) -> Result<(), JsValue> {
    ensure_nullish(window, PENDING_NAV_SCREEN_KEY, JsValue::NULL)?;
    ensure_array(window, PENDING_SEMANTIC_QUEUE_SEED_KEY)?;
    ensure_array(window, PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY)?;
    ensure_array(window, SEMANTIC_QUEUE_KEY)?;
    ensure_array(window, RUNTIME_STAGE_QUEUE_KEY)?;
    ensure_object(window, SEMANTIC_RESULTS_KEY)?;
    ensure_object(window, RUNTIME_STAGE_RESULTS_KEY)?;
    ensure_semantic_debug(window)?;
    ensure_runtime_stage_debug(window)?;
    ensure_bool(window, SEMANTIC_BUSY_KEY, false)?;
    ensure_bool(window, RUNTIME_STAGE_BUSY_KEY, false)?;
    ensure_bool(window, SEMANTIC_WAKE_SCHEDULED_KEY, false)?;
    ensure_bool(window, RUNTIME_STAGE_WAKE_SCHEDULED_KEY, false)?;
    Ok(())
}

fn ensure_nullish(window: &web_sys::Window, key: &str, default: JsValue) -> Result<(), JsValue> {
    let existing = Reflect::get(window.as_ref(), &JsValue::from_str(key))?;
    if existing.is_null() || existing.is_undefined() {
        Reflect::set(window.as_ref(), &JsValue::from_str(key), &default)?;
    }
    Ok(())
}

fn ensure_bool(window: &web_sys::Window, key: &str, value: bool) -> Result<(), JsValue> {
    let existing = Reflect::get(window.as_ref(), &JsValue::from_str(key))?;
    if existing.is_undefined() {
        Reflect::set(
            window.as_ref(),
            &JsValue::from_str(key),
            &JsValue::from_bool(value),
        )?;
    }
    Ok(())
}

fn ensure_array(window: &web_sys::Window, key: &str) -> Result<Array, JsValue> {
    let existing = Reflect::get(window.as_ref(), &JsValue::from_str(key))?;
    if Array::is_array(&existing) {
        Ok(Array::from(&existing))
    } else {
        let array = Array::new();
        Reflect::set(window.as_ref(), &JsValue::from_str(key), &array)?;
        Ok(array)
    }
}

fn ensure_object(window: &web_sys::Window, key: &str) -> Result<Object, JsValue> {
    let existing = Reflect::get(window.as_ref(), &JsValue::from_str(key))?;
    if existing.is_object() && !existing.is_null() {
        existing
            .dyn_into::<Object>()
            .map_err(|_| JsValue::from_str(&format!("failed to access object window.{key}")))
    } else {
        let object = Object::new();
        Reflect::set(window.as_ref(), &JsValue::from_str(key), &object)?;
        Ok(object)
    }
}

fn ensure_semantic_debug(window: &web_sys::Window) -> Result<(), JsValue> {
    let debug = ensure_object(window, SEMANTIC_DEBUG_KEY)?;
    ensure_object_field(&debug, "last_event", JsValue::from_str("installed"))?;
    ensure_object_field(&debug, "active_command_id", JsValue::NULL)?;
    ensure_object_field(&debug, "last_command_id", JsValue::NULL)?;
    ensure_object_field(&debug, "last_completed_command_id", JsValue::NULL)?;
    ensure_object_field(&debug, "last_result_ok", JsValue::NULL)?;
    ensure_object_field(&debug, "last_error", JsValue::NULL)?;
    ensure_object_field(&debug, "queue_depth", JsValue::from_f64(0.0))?;
    ensure_object_field(&debug, "last_progress_at", js_sys::Date::now().into())?;
    Ok(())
}

fn ensure_runtime_stage_debug(window: &web_sys::Window) -> Result<(), JsValue> {
    let debug = ensure_object(window, RUNTIME_STAGE_DEBUG_KEY)?;
    ensure_object_field(&debug, "last_event", JsValue::from_str("installed"))?;
    ensure_object_field(&debug, "active_command_id", JsValue::NULL)?;
    ensure_object_field(&debug, "last_error", JsValue::NULL)?;
    ensure_object_field(&debug, "queue_depth", JsValue::from_f64(0.0))?;
    ensure_object_field(&debug, "last_progress_at", js_sys::Date::now().into())?;
    Ok(())
}

fn ensure_object_field(object: &Object, key: &str, default: JsValue) -> Result<(), JsValue> {
    let existing = Reflect::get(object.as_ref(), &JsValue::from_str(key))?;
    if existing.is_undefined() {
        Reflect::set(object.as_ref(), &JsValue::from_str(key), &default)?;
    }
    Ok(())
}

fn seed_queue_from_window<T>(
    window: &web_sys::Window,
    key: &str,
    mut accept: impl FnMut(T),
) -> Result<usize, JsValue>
where
    T: for<'de> Deserialize<'de>,
{
    let seed = ensure_array(window, key)?;
    let mut accepted = 0usize;
    for entry in seed.iter() {
        if let Some(payload_json) = entry.as_string() {
            match from_str::<T>(&payload_json) {
                Ok(payload) => {
                    accept(payload);
                    accepted += 1;
                }
                Err(error) => {
                    web_sys::console::warn_1(
                        &format!(
                            "[web-harness-queue] seed parse error key={key} error={error}"
                        )
                        .into(),
                    );
                }
            }
        }
    }
    Reflect::set(window.as_ref(), &JsValue::from_str(key), &Array::new())?;
    Ok(accepted)
}

fn install_semantic_enqueue(window: &web_sys::Window) -> Result<(), JsValue> {
    let enqueue = Closure::wrap(Box::new(move |payload: JsValue| -> JsValue {
        if let Some(payload_json) = payload.as_string() {
            match from_str::<SemanticQueuePayload>(&payload_json) {
                Ok(payload) => {
                    let command_id = payload.command_id.clone();
                    let queue_depth = SEMANTIC_QUEUE_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        state.queue.push_back(payload);
                        state.queue.len()
                    });
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness-queue] semantic enqueue command_id={command_id} depth={queue_depth}"
                        )
                        .into(),
                    );
                    sync_semantic_queue_state_to_window();
                    set_semantic_debug("enqueued", None, None, None, Some(queue_depth), None);
                    wake_semantic_queue(0);
                }
                Err(error) => {
                    set_semantic_debug(
                        "enqueue_parse_error",
                        None,
                        None,
                        None,
                        None,
                        Some(&error.to_string()),
                    );
                }
            }
        }
        queue_status_snapshot(SEMANTIC_DEBUG_KEY, || {
            SEMANTIC_QUEUE_STATE.with(|state| state.borrow().queue.len())
        })
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_ENQUEUE_KEY),
        enqueue.as_ref().unchecked_ref(),
    )?;
    enqueue.forget();
    Ok(())
}

fn install_runtime_stage_enqueue(window: &web_sys::Window) -> Result<(), JsValue> {
    let enqueue = Closure::wrap(Box::new(move |payload: JsValue| -> JsValue {
        if let Some(payload_json) = payload.as_string() {
            match from_str::<RuntimeStageQueuePayload>(&payload_json) {
                Ok(payload) => {
                    let command_id = payload.command_id.clone();
                    let queue_depth = RUNTIME_STAGE_QUEUE_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        state.queue.push_back(payload);
                        state.queue.len()
                    });
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness-queue] runtime-stage enqueue command_id={command_id} depth={queue_depth}"
                        )
                        .into(),
                    );
                    sync_runtime_stage_queue_state_to_window();
                    set_runtime_stage_debug("enqueued", None, Some(queue_depth), None);
                    wake_runtime_stage_queue(0);
                }
                Err(error) => {
                    set_runtime_stage_debug(
                        "enqueue_parse_error",
                        None,
                        None,
                        Some(&error.to_string()),
                    );
                }
            }
        }
        queue_status_snapshot(RUNTIME_STAGE_DEBUG_KEY, || {
            RUNTIME_STAGE_QUEUE_STATE.with(|state| state.borrow().queue.len())
        })
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(RUNTIME_STAGE_ENQUEUE_KEY),
        enqueue.as_ref().unchecked_ref(),
    )?;
    enqueue.forget();
    Ok(())
}

fn install_wake_semantic_queue(window: &web_sys::Window) -> Result<(), JsValue> {
    let wake = Closure::wrap(Box::new(move |delay_ms: JsValue| {
        let delay_ms = delay_ms.as_f64().unwrap_or(0.0).max(0.0) as i32;
        wake_semantic_queue(delay_ms);
    }) as Box<dyn FnMut(JsValue)>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(WAKE_SEMANTIC_QUEUE_KEY),
        wake.as_ref().unchecked_ref(),
    )?;
    wake.forget();
    Ok(())
}

fn install_wake_runtime_stage_queue(window: &web_sys::Window) -> Result<(), JsValue> {
    let wake = Closure::wrap(Box::new(move |delay_ms: JsValue| {
        let delay_ms = delay_ms.as_f64().unwrap_or(0.0).max(0.0) as i32;
        wake_runtime_stage_queue(delay_ms);
    }) as Box<dyn FnMut(JsValue)>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(WAKE_RUNTIME_STAGE_QUEUE_KEY),
        wake.as_ref().unchecked_ref(),
    )?;
    wake.forget();
    Ok(())
}

fn install_pending_nav_wake(window: &web_sys::Window) -> Result<(), JsValue> {
    let wake = Closure::wrap(Box::new(move || {
        wake_pending_nav();
    }) as Box<dyn FnMut()>);
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(WAKE_PENDING_NAV_KEY),
        wake.as_ref().unchecked_ref(),
    )?;
    wake.forget();
    Ok(())
}

fn wake_semantic_queue(delay_ms: i32) {
    let should_schedule = SEMANTIC_QUEUE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if delay_ms > 0 {
            if state.wake_scheduled {
                return false;
            }
            state.wake_scheduled = true;
        }
        true
    });
    if !should_schedule {
        return;
    }
    schedule_browser_action(delay_ms, move || {
        SEMANTIC_QUEUE_STATE.with(|state| {
            state.borrow_mut().wake_scheduled = false;
        });
        sync_semantic_queue_state_to_window();
        run_semantic_queue();
    });
}

fn wake_runtime_stage_queue(delay_ms: i32) {
    let should_schedule = RUNTIME_STAGE_QUEUE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if delay_ms > 0 {
            if state.wake_scheduled {
                return false;
            }
            state.wake_scheduled = true;
        }
        true
    });
    if !should_schedule {
        return;
    }
    schedule_browser_action(delay_ms, move || {
        RUNTIME_STAGE_QUEUE_STATE.with(|state| {
            state.borrow_mut().wake_scheduled = false;
        });
        sync_runtime_stage_queue_state_to_window();
        run_runtime_stage_queue();
    });
}

fn wake_pending_nav() {
    let should_schedule = PENDING_NAV_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.wake_scheduled {
            return false;
        }
        state.wake_scheduled = true;
        true
    });
    if !should_schedule {
        return;
    }
    schedule_browser_action(0, move || {
        PENDING_NAV_STATE.with(|state| {
            state.borrow_mut().wake_scheduled = false;
        });
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(value) = Reflect::get(window.as_ref(), &JsValue::from_str(PENDING_NAV_SCREEN_KEY))
        else {
            return;
        };
        let Some(screen_name) = value.as_string() else {
            return;
        };
        let _ = Reflect::set(
            window.as_ref(),
            &JsValue::from_str(PENDING_NAV_SCREEN_KEY),
            &JsValue::NULL,
        );
        let Some(screen) = classify_screen_item_id(&screen_name) else {
            return;
        };
        let Some(controller) = current_controller() else {
            return;
        };
        apply_browser_ui_mutation(controller, move |controller| {
            controller.set_screen(screen);
        });
    });
}

fn run_semantic_queue() {
    if semantic_submit_surface_state().status() != PublicationStatus::Ready {
        set_semantic_debug("queue_wait_ready", None, None, None, None, None);
        wake_semantic_queue(25);
        return;
    }
    let Some(controller) = current_controller() else {
        set_semantic_debug("queue_wait_bridge", None, None, None, None, None);
        wake_semantic_queue(25);
        return;
    };
    let next = SEMANTIC_QUEUE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.busy || state.queue.is_empty() {
            return None;
        }
        state.busy = true;
        state.queue.pop_front()
    });
    sync_semantic_queue_state_to_window();
    let Some(next) = next else {
        return;
    };
    let command_id = next.command_id.clone();
    let queue_depth = SEMANTIC_QUEUE_STATE.with(|state| state.borrow().queue.len());
    web_sys::console::log_1(
        &format!(
            "[web-harness-queue] semantic run start command_id={command_id} remaining_depth={queue_depth}"
        )
        .into(),
    );
    set_semantic_debug(
        "queue_start",
        Some(&command_id),
        Some(&command_id),
        None,
        Some(queue_depth),
        None,
    );
    shared_web_task_owner().spawn_local(async move {
        set_semantic_debug(
            "queue_invoke",
            Some(&command_id),
            Some(&command_id),
            None,
            None,
            None,
        );
        let outcome = run_semantic_payload(controller, &next).await;
        finish_semantic_run(&command_id, outcome);
    });
}

async fn run_semantic_payload(
    controller: std::sync::Arc<aura_ui::UiController>,
    payload: &SemanticQueuePayload,
) -> Result<JsValue, String> {
    let request = commands::BrowserSemanticBridgeRequest::from_json(&payload.request_json)
        .map_err(|error| error.as_string().unwrap_or_else(|| format!("{error:?}")))?;
    let response = request
        .submit(controller)
        .await
        .map_err(|error| {
            error
                .as_string()
                .unwrap_or_else(|| format!("{error:?}"))
        })?;
    response
        .into_json()
        .map(|response_json| JsValue::from_str(&response_json))
        .map_err(|error| error.as_string().unwrap_or_else(|| format!("{error:?}")))
}

fn finish_semantic_run(command_id: &str, outcome: Result<JsValue, String>) {
    SEMANTIC_QUEUE_STATE.with(|state| {
        state.borrow_mut().busy = false;
    });
    sync_semantic_queue_state_to_window();
    match outcome {
        Ok(result) => {
            web_sys::console::log_1(
                &format!("[web-harness-queue] semantic run ok command_id={command_id}").into(),
            );
            push_result(
                SEMANTIC_RESULTS_KEY,
                command_id,
                true,
                Some(result.clone()),
                None,
            );
            notify_push_function(
                PUSH_SEMANTIC_RESULT_KEY,
                command_id,
                true,
                Some(normalize_driver_result_payload(&result)),
                None,
            );
            set_semantic_debug(
                "queue_ok",
                None,
                None,
                Some(command_id),
                None,
                None,
            );
            set_semantic_last_result_ok(Some(true));
        }
        Err(error) => {
            web_sys::console::error_1(
                &format!(
                    "[web-harness-queue] semantic run error command_id={command_id} error={error}"
                )
                .into(),
            );
            push_result(SEMANTIC_RESULTS_KEY, command_id, false, None, Some(&error));
            notify_push_function(
                PUSH_SEMANTIC_RESULT_KEY,
                command_id,
                false,
                None,
                Some(&error),
            );
            set_semantic_debug(
                "queue_error",
                None,
                None,
                Some(command_id),
                None,
                Some(&error),
            );
            set_semantic_last_result_ok(Some(false));
        }
    }
    wake_semantic_queue(0);
}

fn run_runtime_stage_queue() {
    let next = RUNTIME_STAGE_QUEUE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.busy || state.queue.is_empty() {
            return None;
        }
        state.busy = true;
        state.queue.pop_front()
    });
    sync_runtime_stage_queue_state_to_window();
    let Some(next) = next else {
        return;
    };
    let command_id = next.command_id.clone();
    let queue_depth = RUNTIME_STAGE_QUEUE_STATE.with(|state| state.borrow().queue.len());
    set_runtime_stage_debug("queue_start", Some(&command_id), Some(queue_depth), None);
    shared_web_task_owner().spawn_local(async move {
        set_runtime_stage_debug("queue_invoke", Some(&command_id), None, None);
        let outcome = run_runtime_stage_payload(&next).await;
        finish_runtime_stage_run(&command_id, outcome);
    });
}

async fn run_runtime_stage_payload(payload: &RuntimeStageQueuePayload) -> Result<JsValue, String> {
    let _ = from_str::<BootstrapRuntimeIdentity>(&payload.runtime_identity_json)
        .map_err(|error| format!("invalid staged runtime identity payload: {error}"))?;
    crate::harness_bridge::stage_runtime_identity(payload.runtime_identity_json.clone())
        .await
        .map_err(|error| {
            error
                .as_string()
                .unwrap_or_else(|| format!("{error:?}"))
        })?;
    Ok(JsValue::UNDEFINED)
}

fn finish_runtime_stage_run(command_id: &str, outcome: Result<JsValue, String>) {
    RUNTIME_STAGE_QUEUE_STATE.with(|state| {
        state.borrow_mut().busy = false;
    });
    sync_runtime_stage_queue_state_to_window();
    match outcome {
        Ok(result) => {
            web_sys::console::log_1(
                &format!(
                    "[web-harness-queue] runtime-stage run ok command_id={command_id}"
                )
                .into(),
            );
            push_result(
                RUNTIME_STAGE_RESULTS_KEY,
                command_id,
                true,
                Some(result),
                None,
            );
            notify_push_function(
                PUSH_RUNTIME_STAGE_RESULT_KEY,
                command_id,
                true,
                Some(JsValue::NULL),
                None,
            );
            set_runtime_stage_debug("queue_ok", None, None, None);
        }
        Err(error) => {
            web_sys::console::error_1(
                &format!(
                    "[web-harness-queue] runtime-stage run error command_id={command_id} error={error}"
                )
                .into(),
            );
            push_result(
                RUNTIME_STAGE_RESULTS_KEY,
                command_id,
                false,
                None,
                Some(&error),
            );
            notify_push_function(
                PUSH_RUNTIME_STAGE_RESULT_KEY,
                command_id,
                false,
                None,
                Some(&error),
            );
            set_runtime_stage_debug("queue_error", None, None, Some(&error));
        }
    }
    wake_runtime_stage_queue(0);
}

fn push_result(
    result_key: &str,
    command_id: &str,
    ok: bool,
    result: Option<JsValue>,
    error: Option<&str>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(results) = ensure_object(&window, result_key) else {
        return;
    };
    let payload = Object::new();
    let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("ok"), &JsValue::from_bool(ok));
    match result {
        Some(result) => {
            let normalized = normalize_driver_result_payload(&result);
            let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("result"), &normalized);
        }
        None => {
            let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("result"), &JsValue::NULL);
        }
    }
    if let Some(error) = error {
        let _ = Reflect::set(
            payload.as_ref(),
            &JsValue::from_str("error"),
            &JsValue::from_str(error),
        );
    }
    let _ = Reflect::set(results.as_ref(), &JsValue::from_str(command_id), &payload);
}

fn notify_push_function(
    function_key: &str,
    command_id: &str,
    ok: bool,
    result: Option<JsValue>,
    error: Option<&str>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(function) = window_function(&window, function_key) else {
        web_sys::console::warn_1(
            &format!(
                "[web-harness-queue] push function missing key={function_key} command_id={command_id}"
            )
            .into(),
        );
        return;
    };
    let payload = Object::new();
    let _ = Reflect::set(
        payload.as_ref(),
        &JsValue::from_str("command_id"),
        &JsValue::from_str(command_id),
    );
    let _ = Reflect::set(
        payload.as_ref(),
        &JsValue::from_str("ok"),
        &JsValue::from_bool(ok),
    );
    match result {
        Some(result) => {
            let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("result"), &result);
        }
        None => {
            let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("result"), &JsValue::NULL);
        }
    }
    if let Some(error) = error {
        let _ = Reflect::set(
            payload.as_ref(),
            &JsValue::from_str("error"),
            &JsValue::from_str(error),
        );
    }
    let _ = function.call1(window.as_ref(), &payload);
}

fn normalize_driver_result_payload(value: &JsValue) -> JsValue {
    if value.is_null() || value.is_undefined() {
        return JsValue::NULL;
    }
    match JSON::stringify(value)
        .ok()
        .and_then(|json| json.as_string())
        .and_then(|json| JSON::parse(&json).ok())
    {
        Some(parsed) => parsed,
        None => {
            let payload = Object::new();
            let _ = Reflect::set(
                payload.as_ref(),
                &JsValue::from_str("__driver_result_normalization_error"),
                &JsValue::from_str("failed to normalize driver result payload"),
            );
            payload.into()
        }
    }
}

fn schedule_browser_action(delay_ms: i32, action: impl FnOnce() + 'static) {
    let Some(window) = web_sys::window() else {
        action();
        return;
    };
    let action = std::rc::Rc::new(RefCell::new(Some(Box::new(action) as Box<dyn FnOnce()>)));
    if delay_ms <= 0 {
        let fallback_action = action.clone();
        let schedule_result = schedule_browser_task_next_tick(move || {
            if let Some(action) = fallback_action.borrow_mut().take() {
                action();
            }
        });
        if let Err(error) = schedule_result {
            web_sys::console::warn_1(
                &format!(
                    "[web-harness-queue] next-tick scheduling failed; executing inline error={error:?}"
                )
                .into(),
            );
            if let Some(action) = action.borrow_mut().take() {
                action();
            }
        }
        return;
    }
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
    });
    if window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            delay_ms,
        )
        .is_ok()
    {
        callback.forget();
    } else if let Some(action) = action.borrow_mut().take() {
        web_sys::console::warn_1(
            &format!(
                "[web-harness-queue] delayed scheduling failed; executing inline delay_ms={delay_ms}"
            )
            .into(),
        );
        action();
    }
}

fn sync_semantic_queue_state_to_window() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let (queue_depth, busy, wake_scheduled) = SEMANTIC_QUEUE_STATE.with(|state| {
        let state = state.borrow();
        (state.queue.len(), state.busy, state.wake_scheduled)
    });
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_BUSY_KEY),
        &JsValue::from_bool(busy),
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_WAKE_SCHEDULED_KEY),
        &JsValue::from_bool(wake_scheduled),
    );
    if let Ok(queue) = ensure_array(&window, SEMANTIC_QUEUE_KEY) {
        queue.set_length(0);
        for _ in 0..queue_depth {
            queue.push(&JsValue::from_str("queued"));
        }
    }
    set_semantic_debug("state_sync", None, None, None, Some(queue_depth), None);
}

fn sync_runtime_stage_queue_state_to_window() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let (queue_depth, busy, wake_scheduled) = RUNTIME_STAGE_QUEUE_STATE.with(|state| {
        let state = state.borrow();
        (state.queue.len(), state.busy, state.wake_scheduled)
    });
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(RUNTIME_STAGE_BUSY_KEY),
        &JsValue::from_bool(busy),
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(RUNTIME_STAGE_WAKE_SCHEDULED_KEY),
        &JsValue::from_bool(wake_scheduled),
    );
    if let Ok(queue) = ensure_array(&window, RUNTIME_STAGE_QUEUE_KEY) {
        queue.set_length(0);
        for _ in 0..queue_depth {
            queue.push(&JsValue::from_str("queued"));
        }
    }
    set_runtime_stage_debug("state_sync", None, Some(queue_depth), None);
}

fn set_semantic_debug(
    event: &str,
    active_command_id: Option<&str>,
    last_command_id: Option<&str>,
    last_completed_command_id: Option<&str>,
    queue_depth: Option<usize>,
    error: Option<&str>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(debug) = ensure_object(&window, SEMANTIC_DEBUG_KEY) else {
        return;
    };
    let _ = Reflect::set(
        debug.as_ref(),
        &JsValue::from_str("last_event"),
        &JsValue::from_str(event),
    );
    if let Some(active_command_id) = active_command_id {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("active_command_id"),
            &JsValue::from_str(active_command_id),
        );
    } else if matches!(event, "queue_ok" | "queue_error" | "queue_wait_ready" | "queue_wait_bridge")
    {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("active_command_id"),
            &JsValue::NULL,
        );
    }
    if let Some(last_command_id) = last_command_id {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_command_id"),
            &JsValue::from_str(last_command_id),
        );
    }
    if let Some(last_completed_command_id) = last_completed_command_id {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_completed_command_id"),
            &JsValue::from_str(last_completed_command_id),
        );
    }
    if let Some(queue_depth) = queue_depth {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("queue_depth"),
            &JsValue::from_f64(queue_depth as f64),
        );
    }
    let _ = Reflect::set(
        debug.as_ref(),
        &JsValue::from_str("last_progress_at"),
        &js_sys::Date::now().into(),
    );
    if let Some(error) = error {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_error"),
            &JsValue::from_str(error),
        );
    } else if event != "queue_error" && event != "enqueue_parse_error" {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_error"),
            &JsValue::NULL,
        );
    }
}

fn set_semantic_last_result_ok(result: Option<bool>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(debug) = ensure_object(&window, SEMANTIC_DEBUG_KEY) else {
        return;
    };
    let value = match result {
        Some(result) => JsValue::from_bool(result),
        None => JsValue::NULL,
    };
    let _ = Reflect::set(debug.as_ref(), &JsValue::from_str("last_result_ok"), &value);
}

fn set_runtime_stage_debug(
    event: &str,
    active_command_id: Option<&str>,
    queue_depth: Option<usize>,
    error: Option<&str>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(debug) = ensure_object(&window, RUNTIME_STAGE_DEBUG_KEY) else {
        return;
    };
    let _ = Reflect::set(
        debug.as_ref(),
        &JsValue::from_str("last_event"),
        &JsValue::from_str(event),
    );
    if let Some(active_command_id) = active_command_id {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("active_command_id"),
            &JsValue::from_str(active_command_id),
        );
    } else if matches!(event, "queue_ok" | "queue_error") {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("active_command_id"),
            &JsValue::NULL,
        );
    }
    if let Some(queue_depth) = queue_depth {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("queue_depth"),
            &JsValue::from_f64(queue_depth as f64),
        );
    }
    let _ = Reflect::set(
        debug.as_ref(),
        &JsValue::from_str("last_progress_at"),
        &js_sys::Date::now().into(),
    );
    if let Some(error) = error {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_error"),
            &JsValue::from_str(error),
        );
    } else if event != "queue_error" && event != "enqueue_parse_error" {
        let _ = Reflect::set(
            debug.as_ref(),
            &JsValue::from_str("last_error"),
            &JsValue::NULL,
        );
    }
}

fn queue_status_snapshot(debug_key: &str, depth: impl Fn() -> usize) -> JsValue {
    let Some(window) = web_sys::window() else {
        return JsValue::NULL;
    };
    let payload = Object::new();
    let queue_depth = depth();
    let _ = Reflect::set(
        payload.as_ref(),
        &JsValue::from_str("queue_depth"),
        &JsValue::from_f64(queue_depth as f64),
    );
    if let Ok(debug) = Reflect::get(window.as_ref(), &JsValue::from_str(debug_key)) {
        let _ = Reflect::set(payload.as_ref(), &JsValue::from_str("debug"), &debug);
    }
    payload.into()
}

fn push_current_semantic_submit_state(window: &web_sys::Window) {
    let Some(function) = window_function(window, PUSH_SEMANTIC_SUBMIT_STATE_KEY) else {
        return;
    };
    if let Ok(state) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str(SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY),
    ) {
        let _ = function.call1(window.as_ref(), &state);
    }
}

fn window_function(window: &web_sys::Window, key: &str) -> Option<Function> {
    Reflect::get(window.as_ref(), &JsValue::from_str(key))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
}

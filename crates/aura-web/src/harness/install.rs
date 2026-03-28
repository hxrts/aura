use async_lock::RwLock;
use aura_agent::AuraAgent;
use aura_app::ui::contract::{
    classify_screen_item_id, classify_semantic_settings_section_item_id, ScreenId,
};
use aura_app::ui::scenarios::SemanticCommandRequest;
use aura_app::ui::types::BootstrapRuntimeIdentity;
use aura_app::AppCore;
use aura_ui::{control_selector, UiController};
use js_sys::{Array, Function, Object, Reflect};
use serde_json::{from_str, to_string};
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::future_to_promise;

use crate::harness::commands;
use crate::harness::publication::{
    initialize_harness_publication_state, published_ui_snapshot_value,
    refresh_semantic_submit_surface, semantic_submit_surface_state, PublicationBindingMode,
};
use crate::task_owner::shared_web_task_owner;

fn current_controller() -> Result<Arc<UiController>, JsValue> {
    crate::harness::generation::current_controller()
        .ok_or_else(|| JsValue::from_str("Runtime bridge not available"))
}

pub(crate) fn install_page_owned_mutation_queues(window: &web_sys::Window) -> Result<(), JsValue> {
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
    last_command_id: null,
    last_completed_command_id: null,
    last_result_ok: null,
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
    semanticDebug.last_command_id = next.command_id;
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
        semanticDebug.last_completed_command_id = next.command_id;
        semanticDebug.last_result_ok = true;
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
        semanticDebug.last_completed_command_id = next.command_id;
        semanticDebug.last_result_ok = false;
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

pub(crate) fn install_window_harness_api(
    _harness_transport_context: Option<(Arc<RwLock<AppCore>>, Arc<AuraAgent>)>,
) -> Result<(), JsValue> {
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
                crate::harness_bridge::stage_runtime_identity(serialized_identity).await?;
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
                if let Err(error) = crate::harness_bridge::schedule_browser_ui_mutation(
                    controller,
                    move |controller| {
                        controller.set_screen(target);
                    },
                )
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
                if let Err(error) = crate::harness_bridge::schedule_browser_ui_mutation(
                    controller,
                    move |controller| {
                        controller.set_screen(ScreenId::Settings);
                        controller.set_settings_section(commands::browser_settings_section(target));
                    },
                )
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
                commands::update_semantic_debug("raw_entry", None);
                web_sys::console::log_1(&"[web-harness] submit_semantic_command entry".into());
                let outcome: Result<JsValue, JsValue> = async {
                    let controller = current_controller()?;
                    let request =
                        from_str::<SemanticCommandRequest>(&request_json).map_err(|error| {
                            JsValue::from_str(&format!("invalid semantic command request: {error}"))
                        })?;
                    commands::update_semantic_debug(
                        "raw_parsed",
                        Some(&format!("{:?}", request.intent)),
                    );
                    web_sys::console::log_1(
                        &format!(
                            "[web-harness] submit_semantic_command intent={:?}",
                            request.intent
                        )
                        .into(),
                    );
                    let response = commands::submit_semantic_command(controller, request).await?;
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
                        commands::update_semantic_debug("raw_resolved", None);
                        Ok(value)
                    }
                    Err(error) => {
                        commands::update_semantic_debug(
                            "raw_rejected",
                            error.as_string().as_deref(),
                        );
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

    let process_harness_transport = Closure::wrap(Box::new(move || -> js_sys::Promise {
        future_to_promise(async move {
            crate::shell::run_harness_transport_tick_once().await;
            Ok(JsValue::UNDEFINED)
        })
    }) as Box<dyn FnMut() -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("process_harness_transport"),
        process_harness_transport.as_ref().unchecked_ref(),
    )?;
    process_harness_transport.forget();
    if let Some(window) = web_sys::window() {
        let install_transport_interval = Function::new_no_args(
            r#"
const window = globalThis;
if (window.__AURA_HARNESS_TRANSPORT_INTERVAL_INSTALLED__) {
  return true;
}
window.__AURA_HARNESS_TRANSPORT_INTERVAL_INSTALLED__ = true;
window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__ = false;
window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ = null;
window.setInterval(() => {
  if (window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__) {
    return;
  }
  const processTransport = window.__AURA_HARNESS__?.process_harness_transport;
  if (typeof processTransport !== "function") {
    return;
  }
  window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__ = true;
  if (window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ !== null) {
    window.clearTimeout(window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__);
  }
  window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ = window.setTimeout(() => {
    window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__ = false;
    window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ = null;
  }, 1500);
  let result;
  try {
    result = processTransport();
  } catch (_) {
    window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__ = false;
    if (window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ !== null) {
      window.clearTimeout(window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__);
      window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ = null;
    }
    return;
  }
  Promise.resolve(result)
    .catch(() => {})
    .finally(() => {
      window.__AURA_HARNESS_TRANSPORT_INTERVAL_BUSY__ = false;
      if (window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ !== null) {
        window.clearTimeout(window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__);
        window.__AURA_HARNESS_TRANSPORT_INTERVAL_WATCHDOG__ = null;
      }
    });
}, 100);
return true;
"#,
        );
        let _ = install_transport_interval.call0(window.as_ref());
    }

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
        let app_root_selector = control_selector(
            aura_app::ui::contract::ControlId::AppRoot,
            "ControlId::AppRoot",
        );
        let modal_region_selector = control_selector(
            aura_app::ui::contract::ControlId::ModalRegion,
            "ControlId::ModalRegion",
        );
        let onboarding_root_selector = control_selector(
            aura_app::ui::contract::ControlId::OnboardingRoot,
            "ControlId::OnboardingRoot",
        );
        let toast_region_selector = control_selector(
            aura_app::ui::contract::ControlId::ToastRegion,
            "ControlId::ToastRegion",
        );
        let screen_selector = control_selector(
            aura_app::ui::contract::ControlId::Screen(snapshot.screen),
            "ControlId::Screen(snapshot.screen)",
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
    initialize_harness_publication_state(&window);
    let semantic_submit_state = semantic_submit_surface_state();
    refresh_semantic_submit_surface(&window, PublicationBindingMode::SemanticBridge);
    web_sys::console::log_1(
        &format!(
            "[web-harness] semantic submit surface status={};generation={}",
            semantic_submit_state.status().label(),
            crate::harness::generation::active_generation(),
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

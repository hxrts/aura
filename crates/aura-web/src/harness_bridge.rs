//! JavaScript harness API bridge for browser-based testing.
//!
//! Exposes the UiController to JavaScript via window.harness, enabling the test
//! harness to send keys, capture screenshots, and query UI state from Playwright.

use aura_app::ui::contract::{RenderHeartbeat, UiSnapshot};
use aura_ui::UiController;
use js_sys::{Array, Function, Object, Reflect, JSON};
use serde_wasm_bindgen::to_value;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

thread_local! {
    static CONTROLLER: RefCell<Option<Arc<UiController>>> = const { RefCell::new(None) };
    static LAST_PUBLISHED_UI_STATE_JSON: RefCell<Option<String>> = const { RefCell::new(None) };
    static RENDER_SEQ: RefCell<u64> = const { RefCell::new(0) };
}

fn publish_ui_snapshot_now(
    window: &web_sys::Window,
    value: JsValue,
    json: String,
    screen: aura_app::ui::contract::ScreenId,
    modal: Option<aura_app::ui::contract::ModalId>,
    operation_count: usize,
) {
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_CACHE__"),
        &value,
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_UI_STATE_JSON__"),
        &JsValue::from_str(&json),
    );

    let binding_mode = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_UI_STATE"),
    )
    .ok()
    .and_then(|candidate| candidate.dyn_into::<Function>().ok())
    .map(|function| {
        let _ = function.call1(window.as_ref(), &JsValue::from_str(&json));
        "driver_push"
    })
    .unwrap_or("console_only");

    let should_log = LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        let mut last = slot.borrow_mut();
        if last.as_deref() == Some(json.as_str()) {
            false
        } else {
            *last = Some(json.clone());
            true
        }
    });
    if should_log {
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-publish]binding={binding_mode};screen={screen:?};modal={modal:?};ops={operation_count}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[aura-ui-state]screen={screen:?};modal={modal:?};ops={operation_count};binding={binding_mode}",
        )));
        web_sys::console::log_1(&JsValue::from_str(&format!("[aura-ui-json]{json}")));
    }
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

    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT__"),
        &value,
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_JSON__"),
        &JsValue::from_str(&json),
    );

    if let Ok(function) = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__AURA_DRIVER_PUSH_RENDER_HEARTBEAT"),
    )
    .and_then(|candidate| candidate.dyn_into::<Function>())
    {
        let _ = function.call1(window.as_ref(), &JsValue::from_str(&json));
    }
}

pub fn set_controller(controller: Arc<UiController>) {
    CONTROLLER.with(|slot| {
        *slot.borrow_mut() = Some(controller);
    });
}

pub fn publish_ui_snapshot(snapshot: &UiSnapshot) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(value) = to_value(snapshot) else {
        return;
    };
    let Ok(json) = JSON::stringify(&value) else {
        return;
    };
    let Some(json) = json.as_string() else {
        return;
    };
    let screen = snapshot.screen;
    let modal = snapshot.open_modal;
    let operation_count = snapshot.operations.len();

    // Publish semantic state immediately so harness waits are not gated on
    // the browser reaching the next animation frame.
    publish_ui_snapshot_now(
        &window,
        value.clone(),
        json.clone(),
        screen,
        modal,
        operation_count,
    );

    let raf_window = window.clone();
    let raf_callback = Closure::once_into_js(move || {
        let render_seq = RENDER_SEQ.with(|slot| {
            let mut seq = slot.borrow_mut();
            *seq = seq.saturating_add(1);
            *seq
        });
        publish_render_heartbeat(
            &raf_window,
            &RenderHeartbeat {
                screen,
                open_modal: modal,
                render_seq,
            },
        );
        publish_ui_snapshot_now(&raf_window, value, json, screen, modal, operation_count);
    });
    let raf_function: &Function = raf_callback.unchecked_ref();
    let _ = window.request_animation_frame(raf_function);
}

fn serialize_ui_snapshot(snapshot: &UiSnapshot) -> JsValue {
    match to_value(snapshot)
        .ok()
        .and_then(|value| JSON::stringify(&value).ok())
        .and_then(|value| value.as_string())
    {
        Some(value) => JsValue::from_str(&value),
        None => {
            web_sys::console::error_1(&JsValue::from_str(&format!(
                "failed to serialize UiSnapshot to JSON string"
            )));
            JsValue::NULL
        }
    }
}

pub fn install_window_harness_api(controller: Arc<UiController>) -> Result<(), JsValue> {
    let harness = Object::new();

    let send_keys_controller = controller.clone();
    let send_keys = Closure::wrap(Box::new(move |keys: JsValue| -> JsValue {
        if let Some(text) = keys.as_string() {
            send_keys_controller.send_keys(&text);
        }
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_keys"),
        send_keys.as_ref().unchecked_ref(),
    )?;
    send_keys.forget();

    let send_key_controller = controller.clone();
    let send_key = Closure::wrap(Box::new(move |key: JsValue, repeat: JsValue| -> JsValue {
        let key_name = key.as_string().unwrap_or_default();
        let repeat = repeat
            .as_f64()
            .map(|value| value.max(1.0) as u16)
            .unwrap_or(1);
        send_key_controller.send_key_named(&key_name, repeat);
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("send_key"),
        send_key.as_ref().unchecked_ref(),
    )?;
    send_key.forget();

    let snapshot_controller = controller.clone();
    let snapshot = Closure::wrap(Box::new(move || -> JsValue {
        let rendered = snapshot_controller.snapshot();
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
        &harness,
        &JsValue::from_str("snapshot"),
        snapshot.as_ref().unchecked_ref(),
    )?;
    snapshot.forget();

    let ui_state_controller = controller.clone();
    let ui_state = Closure::wrap(Box::new(move || -> JsValue {
        serialize_ui_snapshot(&ui_state_controller.ui_snapshot())
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("ui_state"),
        ui_state.as_ref().unchecked_ref(),
    )?;
    ui_state.forget();

    let read_clipboard_controller = controller.clone();
    let read_clipboard = Closure::wrap(Box::new(move || -> JsValue {
        JsValue::from_str(&read_clipboard_controller.read_clipboard())
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("read_clipboard"),
        read_clipboard.as_ref().unchecked_ref(),
    )?;
    read_clipboard.forget();

    let authority_id_controller = controller.clone();
    let get_authority_id = Closure::wrap(Box::new(move || -> JsValue {
        JsValue::from_str(&authority_id_controller.authority_id())
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("get_authority_id"),
        get_authority_id.as_ref().unchecked_ref(),
    )?;
    get_authority_id.forget();

    let tail_log_controller = controller.clone();
    let tail_log = Closure::wrap(Box::new(move |lines: JsValue| -> JsValue {
        let lines = lines
            .as_f64()
            .map(|value| value.max(1.0) as usize)
            .unwrap_or(20);
        let array = Array::new();
        for line in tail_log_controller.tail_log(lines) {
            array.push(&JsValue::from_str(&line));
        }
        array.into()
    }) as Box<dyn FnMut(JsValue) -> JsValue>);
    Reflect::set(
        &harness,
        &JsValue::from_str("tail_log"),
        tail_log.as_ref().unchecked_ref(),
    )?;
    tail_log.forget();

    let root_structure_controller = controller.clone();
    let root_structure = Closure::wrap(Box::new(move || -> JsValue {
        let snapshot = root_structure_controller.ui_snapshot();
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
                .unwrap_or("aura-app-root")
        );
        let onboarding_root_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::OnboardingRoot
                .web_dom_id()
                .unwrap_or("aura-onboarding-root")
        );
        let modal_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ModalRegion
                .web_dom_id()
                .unwrap_or("aura-modal-region")
        );
        let toast_region_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::ToastRegion
                .web_dom_id()
                .unwrap_or("aura-toast-region")
        );
        let screen_selector = format!(
            "#{}",
            aura_app::ui::contract::ControlId::Screen(snapshot.screen)
                .web_dom_id()
                .unwrap_or("missing-screen-id")
        );

        let app_root_count = document
            .query_selector_all(&app_root_selector)
            .ok()
            .map(|nodes| nodes.length())
            .unwrap_or(0);
        let modal_region_count = document
            .query_selector_all(&modal_region_selector)
            .ok()
            .map(|nodes| nodes.length())
            .unwrap_or(0);
        let onboarding_root_count = document
            .query_selector_all(&onboarding_root_selector)
            .ok()
            .map(|nodes| nodes.length())
            .unwrap_or(0);
        let toast_region_count = document
            .query_selector_all(&toast_region_selector)
            .ok()
            .map(|nodes| nodes.length())
            .unwrap_or(0);
        let active_screen_root_count = document
            .query_selector_all(&screen_selector)
            .ok()
            .map(|nodes| nodes.length())
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
        &harness,
        &JsValue::from_str("root_structure"),
        root_structure.as_ref().unchecked_ref(),
    )?;
    root_structure.forget();

    let inject_controller = controller.clone();
    let inject_message = Closure::wrap(Box::new(move |message: JsValue| -> JsValue {
        if let Some(text) = message.as_string() {
            inject_controller.inject_message(&text);
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
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str("__AURA_HARNESS__"),
        &harness,
    )?;
    let read_only_ui_state_controller = controller;
    let read_only_ui_state = Closure::wrap(Box::new(move || -> JsValue {
        serialize_ui_snapshot(&read_only_ui_state_controller.ui_snapshot())
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
        window.as_ref(),
        &JsValue::from_str("__AURA_RENDER_HEARTBEAT_STATE__"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    render_heartbeat.forget();

    Ok(())
}

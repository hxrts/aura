use aura_app::ui::contract::{
    classify_screen_item_id, classify_semantic_settings_section_item_id, ScreenId,
};
use aura_app::ui::types::BootstrapRuntimeIdentity;
use aura_ui::{control_selector, UiController};
use js_sys::{Array, Object, Reflect, JSON};
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::future_to_promise;

use crate::harness::browser_contract::{
    HARNESS_API_KEY, HARNESS_CLIPBOARD_KEY, HARNESS_OBSERVE_KEY, RENDER_HEARTBEAT_KEY,
    RENDER_HEARTBEAT_STATE_KEY, UI_STATE_OBSERVE_KEY,
};
use crate::harness::commands;
use crate::harness::page_owned_queue;
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
    page_owned_queue::install(window)
}

pub(crate) fn install_window_harness_api() -> Result<(), JsValue> {
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
            if let Ok(value) =
                Reflect::get(window.as_ref(), &JsValue::from_str(HARNESS_CLIPBOARD_KEY))
            {
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

    let submit_semantic_command_fn =
        Closure::wrap(Box::new(move |request: JsValue| -> js_sys::Promise {
            future_to_promise(async move {
                commands::update_semantic_debug("wrapper_entry", None);
                web_sys::console::log_1(&"[web-harness] submit_semantic_command entry".into());
                let outcome: Result<JsValue, JsValue> = async {
                    let request_json = JSON::stringify(&request)
                        .map_err(|_| {
                            JsValue::from_str(
                                "failed to stringify semantic command request for bridge dispatch",
                            )
                        })?
                        .as_string()
                        .ok_or_else(|| {
                            JsValue::from_str(
                                "semantic command request stringification did not produce a string",
                            )
                        })?;
                    let controller = current_controller()?;
                    let request = commands::BrowserSemanticBridgeRequest::from_json(&request_json)?;
                    commands::update_semantic_debug("wrapper_parsed", Some(&request_json));
                    let response = request.submit(controller).await?;
                    web_sys::console::log_1(&"[web-harness] submit_semantic_command done".into());
                    response.into_js_value()
                }
                .await;

                match outcome {
                    Ok(value) => {
                        commands::update_semantic_debug("wrapper_resolved", None);
                        Ok(value)
                    }
                    Err(error) => {
                        commands::update_semantic_debug(
                            "wrapper_rejected",
                            error.as_string().as_deref(),
                        );
                        Err(error)
                    }
                }
            })
        }) as Box<dyn FnMut(JsValue) -> js_sys::Promise>);
    Reflect::set(
        &harness,
        &JsValue::from_str("submit_semantic_command"),
        submit_semantic_command_fn.as_ref().unchecked_ref(),
    )?;
    submit_semantic_command_fn.forget();

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
        &JsValue::from_str(HARNESS_API_KEY),
        &harness,
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(HARNESS_OBSERVE_KEY),
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
        &JsValue::from_str(UI_STATE_OBSERVE_KEY),
        read_only_ui_state.as_ref().unchecked_ref(),
    )?;
    read_only_ui_state.forget();

    let render_heartbeat = Closure::wrap(Box::new(move || -> JsValue {
        let window = match web_sys::window() {
            Some(window) => window,
            None => return JsValue::NULL,
        };
        Reflect::get(window.as_ref(), &JsValue::from_str(RENDER_HEARTBEAT_KEY))
            .unwrap_or(JsValue::NULL)
    }) as Box<dyn FnMut() -> JsValue>);
    Reflect::set(
        &observe,
        &JsValue::from_str("render_heartbeat"),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    Reflect::set(
        window.as_ref(),
        &JsValue::from_str(RENDER_HEARTBEAT_STATE_KEY),
        render_heartbeat.as_ref().unchecked_ref(),
    )?;
    render_heartbeat.forget();

    Ok(())
}

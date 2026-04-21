//! Web Clipboard API adapter for browser environments.
//!
//! Implements the ClipboardPort trait using the browser's Clipboard API,
//! with a synchronous mirror for read operations that can't await async results.

use crate::browser_promises::await_browser_promise_with_timeout;
use crate::error::{log_web_error, WebUiError};
use aura_app::frontend_primitives::{ClipboardPort, FrontendUiOperation as WebUiOperation};
use js_sys::Reflect;
use std::sync::RwLock;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, HtmlElement, HtmlTextAreaElement, Window};

#[derive(Default)]
pub struct WebClipboardAdapter {
    mirror: RwLock<String>,
}

impl ClipboardPort for WebClipboardAdapter {
    fn write(&self, text: &str) {
        match self.mirror.write() {
            Ok(mut guard) => {
                *guard = text.to_string();
            }
            Err(error) => {
                log_web_error(
                    "warn",
                    &WebUiError::operation(
                        WebUiOperation::WriteSystemClipboard,
                        "WEB_CLIPBOARD_MIRROR_WRITE_LOCK_FAILED",
                        format!("failed to lock clipboard mirror for write: {error}"),
                    ),
                );
            }
        }

        if let Some(window) = web_sys::window() {
            if let Err(error) = Reflect::set(
                window.as_ref(),
                &JsValue::from_str("__AURA_HARNESS_CLIPBOARD__"),
                &JsValue::from_str(text),
            ) {
                log_web_error(
                    "warn",
                    &WebUiError::operation(
                        WebUiOperation::MirrorClipboardToHarness,
                        "WEB_HARNESS_CLIPBOARD_MIRROR_FAILED",
                        format!("failed to mirror clipboard into harness window state: {error:?}"),
                    ),
                );
            }
            if let Ok(push) = Reflect::get(
                window.as_ref(),
                &JsValue::from_str("__AURA_DRIVER_PUSH_CLIPBOARD"),
            ) {
                if let Some(function) = push.dyn_ref::<js_sys::Function>() {
                    if let Err(error) = function.call1(window.as_ref(), &JsValue::from_str(text)) {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::NotifyHarnessClipboardDriver,
                                "WEB_HARNESS_CLIPBOARD_DRIVER_NOTIFY_FAILED",
                                format!("failed to notify harness clipboard driver: {error:?}"),
                            ),
                        );
                    }
                }
            }
        }

        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let clipboard = navigator.clipboard();
            let text = text.to_string();
            let fallback_succeeded = match try_exec_command_copy(&window, &text) {
                Ok(copied) => copied,
                Err(error) => {
                    log_web_error("warn", &error);
                    false
                }
            };
            // Start the browser clipboard write synchronously so user-triggered
            // copy actions retain the activation required by navigator.clipboard.
            let promise = clipboard.write_text(&text);
            spawn_local(async move {
                if let Err(error) = await_browser_promise_with_timeout(
                    promise,
                    5_000,
                    WebUiOperation::WriteSystemClipboard,
                    "WEB_SYSTEM_CLIPBOARD_WRITE_REJECTED",
                    "WEB_SYSTEM_CLIPBOARD_WRITE_TIMEOUT",
                    "WEB_SYSTEM_CLIPBOARD_WRITE_TIMEOUT_SCHEDULE_FAILED",
                    "WEB_SYSTEM_CLIPBOARD_WRITE_TIMEOUT_DROPPED",
                    "system clipboard write",
                    None,
                )
                .await
                {
                    if !fallback_succeeded {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::WriteSystemClipboard,
                                "WEB_SYSTEM_CLIPBOARD_WRITE_FAILED",
                                error.user_message(),
                            ),
                        );
                    }
                }
            });
        }
    }

    fn read(&self) -> String {
        match self.mirror.read() {
            Ok(guard) => {
                if !guard.is_empty() {
                    return guard.clone();
                }
            }
            Err(error) => {
                log_web_error(
                    "warn",
                    &WebUiError::operation(
                        WebUiOperation::WriteSystemClipboard,
                        "WEB_CLIPBOARD_MIRROR_READ_LOCK_FAILED",
                        format!("failed to lock clipboard mirror for read: {error}"),
                    ),
                );
            }
        }
        if let Some(window) = web_sys::window() {
            if let Ok(value) = Reflect::get(
                window.as_ref(),
                &JsValue::from_str("__AURA_HARNESS_CLIPBOARD__"),
            ) {
                if let Some(text) = value.as_string() {
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }
        String::new()
    }
}

fn try_exec_command_copy(window: &Window, text: &str) -> Result<bool, WebUiError> {
    let Some(document) = window.document() else {
        return Ok(false);
    };
    let Some(body) = document.body() else {
        return Ok(false);
    };

    let textarea = document
        .create_element("textarea")
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_CREATE_FAILED", error))?
        .dyn_into::<HtmlTextAreaElement>()
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_CAST_FAILED", error.into()))?;
    textarea.set_value(text);
    set_clipboard_fallback_attr(&textarea, "readonly", "readonly")?;
    set_clipboard_fallback_attr(&textarea, "aria-hidden", "true")?;
    set_clipboard_fallback_attr(&textarea, "tabindex", "-1")?;
    set_clipboard_fallback_attr(
        &textarea,
        "style",
        "position: fixed; top: -1000px; left: -1000px; opacity: 0; pointer-events: none;",
    )?;

    let previous_focus = document
        .active_element()
        .and_then(|element| element.dyn_into::<HtmlElement>().ok());

    body.append_child(&textarea)
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_ATTACH_FAILED", error))?;
    textarea.select();
    let copied = exec_copy_command(&document);
    textarea.remove();
    if let Some(active_element) = previous_focus {
        let _ = active_element.focus();
    }
    copied
}

fn set_clipboard_fallback_attr(
    textarea: &HtmlTextAreaElement,
    name: &str,
    value: &str,
) -> Result<(), WebUiError> {
    textarea
        .set_attribute(name, value)
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_ATTR_FAILED", error))
}

fn exec_copy_command(document: &Document) -> Result<bool, WebUiError> {
    let exec_command = Reflect::get(document.as_ref(), &JsValue::from_str("execCommand"))
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_EXEC_LOOKUP_FAILED", error))?
        .dyn_into::<js_sys::Function>()
        .map_err(|error| {
            clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_EXEC_CAST_FAILED", error.into())
        })?;
    let result = exec_command
        .call1(document.as_ref(), &JsValue::from_str("copy"))
        .map_err(|error| clipboard_dom_error("WEB_CLIPBOARD_FALLBACK_EXEC_FAILED", error))?;
    Ok(result.as_bool().unwrap_or(false))
}

fn clipboard_dom_error(code: &'static str, error: JsValue) -> WebUiError {
    WebUiError::operation(
        WebUiOperation::WriteSystemClipboard,
        code,
        format!("legacy clipboard fallback failed: {error:?}"),
    )
}

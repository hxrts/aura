//! Web Clipboard API adapter for browser environments.
//!
//! Implements the ClipboardPort trait using the browser's Clipboard API,
//! with a synchronous mirror for read operations that can't await async results.

use crate::browser_promises::await_browser_promise_with_timeout;
use crate::error::{log_web_error, WebUiError};
use aura_ui::ClipboardPort;
use aura_ui::FrontendUiOperation as WebUiOperation;
use js_sys::Reflect;
use std::sync::RwLock;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

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
            spawn_local(async move {
                let promise = clipboard.write_text(&text);
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
                    log_web_error(
                        "warn",
                        &WebUiError::operation(
                            WebUiOperation::WriteSystemClipboard,
                            "WEB_SYSTEM_CLIPBOARD_WRITE_FAILED",
                            error.user_message(),
                        ),
                    );
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

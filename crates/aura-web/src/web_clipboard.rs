//! Web Clipboard API adapter for browser environments.
//!
//! Implements the ClipboardPort trait using the browser's Clipboard API,
//! with a synchronous mirror for read operations that can't await async results.

use aura_ui::ClipboardPort;
use js_sys::Reflect;
use std::sync::RwLock;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};

#[derive(Default)]
pub struct WebClipboardAdapter {
    mirror: RwLock<String>,
}

impl ClipboardPort for WebClipboardAdapter {
    fn write(&self, text: &str) {
        if let Ok(mut guard) = self.mirror.write() {
            *guard = text.to_string();
        }

        if let Some(window) = web_sys::window() {
            let _ = Reflect::set(
                window.as_ref(),
                &JsValue::from_str("__AURA_HARNESS_CLIPBOARD__"),
                &JsValue::from_str(text),
            );
        }

        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let clipboard = navigator.clipboard();
            let text = text.to_string();
            spawn_local(async move {
                let promise = clipboard.write_text(&text);
                let _ = JsFuture::from(promise).await;
            });
        }
    }

    fn read(&self) -> String {
        if let Ok(guard) = self.mirror.read() {
            if !guard.is_empty() {
                return guard.clone();
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

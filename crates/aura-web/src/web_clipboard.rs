use aura_ui::ClipboardPort;
use std::sync::RwLock;
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
            return guard.clone();
        }
        String::new()
    }
}

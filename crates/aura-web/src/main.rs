//! Aura web application entry point for WASM targets.
//!
//! Initializes the Dioxus-based web UI with the AppCore, clipboard adapter,
//! and harness bridge for browser-based deployment and testing.

#![allow(missing_docs)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod harness_bridge;
        mod web_clipboard;

        use async_lock::RwLock;
        use aura_app::{AppConfig, AppCore};
        use aura_ui::{AuraUiRoot, UiController};
        use dioxus::prelude::*;
        use std::sync::Arc;
        use web_clipboard::WebClipboardAdapter;

        fn main() {
            aura_app::platform::wasm::initialize();

            let app_core = match AppCore::new(AppConfig::default()) {
                Ok(core) => Arc::new(RwLock::new(core)),
                Err(error) => {
                    web_sys::console::error_1(&format!("failed to initialize AppCore: {error}").into());
                    return;
                }
            };

            let clipboard = Arc::new(WebClipboardAdapter::default());
            let controller = Arc::new(UiController::new(app_core, clipboard));
            harness_bridge::set_controller(controller.clone());

            if let Err(error) = harness_bridge::install_window_harness_api(controller.clone()) {
                web_sys::console::error_1(&format!("failed to install harness API: {error:?}").into());
            }

            controller.push_log("runtime bootstrap disabled in web shell");
            dioxus::launch(App);
        }

        #[component]
        fn App() -> Element {
            if let Some(controller) = harness_bridge::controller() {
                return rsx! {
                    AuraUiRoot {
                        controller: controller.clone(),
                    }
                };
            }

            rsx! {
                main {
                    h1 { "Aura" }
                    p { "Harness bridge not initialized" }
                }
            }
        }
    } else {
        fn main() {
            eprintln!("aura-web is a wasm32 frontend. Build with target wasm32-unknown-unknown.");
        }
    }
}

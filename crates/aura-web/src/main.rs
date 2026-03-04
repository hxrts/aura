#![allow(missing_docs)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod harness_bridge;
        mod web_clipboard;

        use async_lock::RwLock;
        use aura_agent::AgentBuilder;
        use aura_app::{AppConfig, AppCore};
        use aura_ui::{AuraUiRoot, UiController};
        use dioxus::prelude::*;
        use std::sync::Arc;
        use wasm_bindgen_futures::spawn_local;
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

            spawn_runtime_bootstrap(controller);
            dioxus::launch(App);
        }

        fn spawn_runtime_bootstrap(controller: Arc<UiController>) {
            spawn_local(async move {
                controller.push_log("runtime bootstrap: starting");
                match AgentBuilder::web().build().await {
                    Ok(agent) => {
                        let authority_id = agent.authority_id();
                        controller.set_authority_id(&authority_id.to_string());
                        controller.push_log("runtime bootstrap: agent ready");

                        let agent = Arc::new(agent);
                        match AppCore::with_runtime(AppConfig::default(), agent.clone().as_runtime_bridge()) {
                            Ok(core) => {
                                let runtime_core = Arc::new(RwLock::new(core));
                                if let Err(error) = AppCore::init_signals_with_hooks(&runtime_core).await {
                                    controller.push_log(&format!("runtime bootstrap: signal init failed: {error}"));
                                } else {
                                    controller.push_log("runtime bootstrap: signal hooks installed");
                                }
                            }
                            Err(error) => controller.push_log(&format!("runtime bootstrap: app core wiring failed: {error}")),
                        }
                    }
                    Err(error) => controller.push_log(&format!("runtime bootstrap failed: {error}")),
                }
            });
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
                    h1 { "Aura Web" }
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

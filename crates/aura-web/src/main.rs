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
        use aura_agent::AgentBuilder;
        use aura_app::{AppConfig, AppCore};
        use aura_core::identifiers::AuthorityId;
        use aura_ui::{AuraUiRoot, UiController};
        use dioxus::prelude::*;
        use std::sync::Arc;
        use web_clipboard::WebClipboardAdapter;

        const WEB_STORAGE_PREFIX: &str = "aura_";
        const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";

        fn selected_authority_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}selected_authority")
        }

        fn sanitize_storage_segment(raw: &str) -> String {
            raw.chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        }

        fn harness_instance_id() -> Option<String> {
            let window = web_sys::window()?;
            let search = window.location().search().ok()?;
            let query = search.strip_prefix('?').unwrap_or(&search);
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    if key == HARNESS_INSTANCE_QUERY_KEY && !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
            None
        }

        fn active_storage_prefix() -> String {
            if let Some(instance_id) = harness_instance_id() {
                let sanitized = sanitize_storage_segment(&instance_id);
                if !sanitized.is_empty() {
                    return format!("{WEB_STORAGE_PREFIX}{sanitized}_");
                }
            }
            WEB_STORAGE_PREFIX.to_string()
        }

        fn load_selected_authority(storage_key: &str) -> Option<AuthorityId> {
            let window = web_sys::window()?;
            let storage = window.local_storage().ok().flatten()?;
            let raw = storage.get_item(storage_key).ok().flatten()?;
            raw.parse::<AuthorityId>().ok()
        }

        fn persist_selected_authority(
            storage_key: &str,
            authority_id: &AuthorityId,
        ) -> Result<(), String> {
            let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
            let storage = window
                .local_storage()
                .map_err(|error| format!("localStorage unavailable: {:?}", error))?
                .ok_or_else(|| "localStorage unavailable".to_string())?;
            storage
                .set_item(storage_key, &authority_id.to_string())
                .map_err(|error| format!("failed to persist selected authority: {:?}", error))
        }

        fn reload_page() -> Result<(), String> {
            let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
            window
                .location()
                .reload()
                .map_err(|error| format!("failed to reload page: {:?}", error))
        }

        async fn bootstrap_controller() -> Result<Arc<UiController>, String> {
            let storage_prefix = active_storage_prefix();
            let authority_storage_key = selected_authority_key(&storage_prefix);
            let selected_authority = load_selected_authority(&authority_storage_key);
            let builder = AgentBuilder::web().storage_prefix(&storage_prefix);
            let builder = if let Some(authority_id) = selected_authority {
                builder.authority(authority_id)
            } else {
                builder
            };

            let agent = Arc::new(
                builder
                    .build()
                    .await
                    .map_err(|error| format!("failed to build web runtime: {error}"))?,
            );

            let current_authority = agent.authority_id().clone();
            if let Err(error) = persist_selected_authority(&authority_storage_key, &current_authority)
            {
                web_sys::console::warn_1(
                    &format!("failed to persist selected authority: {error}").into(),
                );
            }

            let app_core = Arc::new(RwLock::new(
                AppCore::with_runtime(AppConfig::default(), agent.as_runtime_bridge())
                    .map_err(|error| format!("failed to initialize AppCore: {error}"))?,
            ));

            AppCore::init_signals_with_hooks(&app_core)
                .await
                .map_err(|error| format!("failed to initialize app signals: {error}"))?;

            let clipboard = Arc::new(WebClipboardAdapter::default());
            let controller = Arc::new(UiController::with_authority_switcher(
                app_core,
                clipboard,
                Some(Arc::new(|authority_id: AuthorityId| {
                    let storage_prefix = active_storage_prefix();
                    let authority_storage_key = selected_authority_key(&storage_prefix);
                    if let Err(error) =
                        persist_selected_authority(&authority_storage_key, &authority_id)
                    {
                        web_sys::console::error_1(
                            &format!("failed to persist authority switch: {error}").into(),
                        );
                        return;
                    }
                    if let Err(error) = reload_page() {
                        web_sys::console::error_1(
                            &format!("failed to reload after authority switch: {error}").into(),
                        );
                    }
                })),
            ));

            harness_bridge::set_controller(controller.clone());
            if let Err(error) = harness_bridge::install_window_harness_api(controller.clone()) {
                web_sys::console::error_1(
                    &format!("failed to install harness API: {error:?}").into(),
                );
            }

            controller.push_log("runtime bootstrap enabled in web shell");
            Ok(controller)
        }

        fn main() {
            aura_app::platform::wasm::initialize();
            let mut tracing_config = tracing_wasm::WASMLayerConfigBuilder::new();
            tracing_config
                .set_max_level(tracing::Level::INFO)
                .set_report_logs_in_timings(false);
            tracing_wasm::set_as_global_default_with_config(tracing_config.build());
            dioxus::launch(App);
        }

        #[component]
        fn App() -> Element {
            let mut bootstrap_state = use_signal(|| None::<Result<Arc<UiController>, String>>);
            let mut bootstrap_started = use_signal(|| false);

            use_effect(|| {
                if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                    document.set_title("Aura");
                }
            });

            use_effect(move || {
                if bootstrap_started() {
                    return;
                }

                bootstrap_started.set(true);
                spawn(async move {
                    bootstrap_state.set(Some(bootstrap_controller().await));
                });
            });

            if let Some(Ok(controller)) = bootstrap_state().as_ref() {
                return rsx! {
                    AuraUiRoot {
                        controller: controller.clone(),
                    }
                };
            }

            if let Some(Err(error)) = bootstrap_state().as_ref() {
                return rsx! {
                    main {
                        class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                        div {
                            class: "max-w-xl space-y-3 text-center",
                            h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                            p { class: "text-sm text-muted-foreground", "Web runtime bootstrap failed." }
                            p { class: "text-xs text-muted-foreground break-words", "{error}" }
                        }
                    }
                };
            }

            rsx! {
                main {
                    class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                    div {
                        class: "max-w-xl space-y-3 text-center",
                        h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                        p { class: "text-sm text-muted-foreground", "Initializing web runtime..." }
                    }
                }
            }
        }
    } else {
        fn main() {
            eprintln!("aura-web is a wasm32 frontend. Build with target wasm32-unknown-unknown.");
        }
    }
}

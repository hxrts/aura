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
        use aura_app::ui::workflows::account as account_workflows;
        use aura_app::ui::workflows::invitation as invitation_workflows;
        use aura_app::ui::workflows::settings as settings_workflows;
        use aura_app::ui::types::InvitationBridgeType;
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

        #[derive(Clone, PartialEq)]
        struct BootstrapState {
            controller: Arc<UiController>,
            account_ready: bool,
        }

        async fn bootstrap_controller() -> Result<BootstrapState, String> {
            let storage_prefix = active_storage_prefix();
            let authority_storage_key = selected_authority_key(&storage_prefix);
            let selected_authority = load_selected_authority(&authority_storage_key);
            let harness_instance = harness_instance_id();
            let builder = AgentBuilder::web().storage_prefix(&storage_prefix);
            let builder = if harness_instance.is_some() {
                builder.testing_mode()
            } else {
                builder
            };
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
            let account_ready = agent
                .clone()
                .as_runtime_bridge()
                .has_account_config()
                .await
                .map_err(|error| format!("failed to load account bootstrap state: {error}"))?;

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
            controller.set_account_setup_state(account_ready, "", None);

            harness_bridge::set_controller(controller.clone());
            if let Err(error) = harness_bridge::install_window_harness_api(controller.clone()) {
                web_sys::console::error_1(
                    &format!("failed to install harness API: {error:?}").into(),
                );
            }

            controller.push_log("runtime bootstrap enabled in web shell");
            if let Some(instance_id) = harness_instance {
                controller.push_log(&format!(
                    "web harness instance {instance_id} booted in testing mode"
                ));
            }
            Ok(BootstrapState {
                controller,
                account_ready,
            })
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
            let bootstrap = use_resource(|| async move { bootstrap_controller().await });

            use_effect(|| {
                if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                    document.set_title("Aura");
                }
            });

            if let Some(Ok(state)) = &*bootstrap.read_unchecked() {
                return rsx! {
                    BootstrappedApp {
                        state: state.clone(),
                    }
                };
            }

            if let Some(Err(error)) = &*bootstrap.read_unchecked() {
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

        #[component]
        fn BootstrappedApp(state: BootstrapState) -> Element {
            let controller = state.controller.clone();
            let account_ready = use_signal(|| state.account_ready);
            let mut account_name = use_signal(String::new);
            let mut account_error = use_signal(|| Option::<String>::None);
            let creating_account = use_signal(|| false);
            let mut import_code = use_signal(String::new);
            let mut import_error = use_signal(|| Option::<String>::None);
            let importing_code = use_signal(|| false);

            if account_ready() {
                controller.set_account_setup_state(true, "", None);
                return rsx! {
                    AuraUiRoot {
                        controller: controller.clone(),
                    }
                };
            }

            let submit_account = {
                let controller = controller.clone();
                let mut account_ready = account_ready.clone();
                let account_name = account_name.clone();
                let mut account_error = account_error.clone();
                let mut creating_account = creating_account.clone();
                move |_| {
                    if creating_account() {
                        return;
                    }

                    let nickname = account_name();
                    creating_account.set(true);
                    account_error.set(None);
                    controller.set_account_setup_state(false, nickname.clone(), None);

                    let controller = controller.clone();
                    spawn(async move {
                        match account_workflows::initialize_runtime_account(
                            controller.app_core(),
                            nickname.clone(),
                        )
                        .await
                        {
                            Ok(()) => {
                                controller.set_account_setup_state(true, "", None);
                                account_ready.set(true);
                                creating_account.set(false);
                            }
                            Err(error) => {
                                let message = error.to_string();
                                controller.set_account_setup_state(
                                    false,
                                    nickname.clone(),
                                    Some(message.clone()),
                                );
                                account_error.set(Some(message));
                                creating_account.set(false);
                            }
                        }
                    });
                }
            };

            let submit_import = {
                let controller = controller.clone();
                let import_code = import_code.clone();
                let mut import_error = import_error.clone();
                let mut importing_code = importing_code.clone();
                let mut account_ready = account_ready.clone();
                move |_| {
                    if importing_code() {
                        return;
                    }

                    let code = import_code();
                    importing_code.set(true);
                    import_error.set(None);

                    let controller = controller.clone();
                    spawn(async move {
                        let app_core = controller.app_core().clone();
                        let result = async {
                            let invitation =
                                invitation_workflows::import_invitation_details(&app_core, &code)
                                    .await?;
                            if !matches!(
                                invitation.invitation_type,
                                InvitationBridgeType::DeviceEnrollment { .. }
                            ) {
                                return Err(aura_core::AuraError::invalid(
                                    "Code is not a device enrollment invitation",
                                ));
                            }

                            invitation_workflows::accept_invitation(
                                &app_core,
                                &invitation.invitation_id,
                            )
                            .await?;
                            settings_workflows::refresh_settings_from_runtime(&app_core).await?;
                            let settings = settings_workflows::get_settings(&app_core).await?;
                            let nickname = settings.nickname_suggestion.trim();
                            let bootstrap_name = if nickname.is_empty() {
                                "Aura User".to_string()
                            } else {
                                nickname.to_string()
                            };
                            account_workflows::initialize_runtime_account(
                                &app_core,
                                bootstrap_name,
                            )
                            .await
                        }
                        .await;

                        match result {
                            Ok(()) => {
                                controller.info_toast("Device enrollment complete");
                                controller.set_account_setup_state(true, "", None);
                                account_ready.set(true);
                                importing_code.set(false);
                            }
                            Err(error) => {
                                let message = error.to_string();
                                controller.set_account_setup_state(
                                    false,
                                    "",
                                    Some(message.clone()),
                                );
                                import_error.set(Some(message));
                                importing_code.set(false);
                            }
                        }
                    });
                }
            };

            rsx! {
                main {
                    id: "aura-onboarding-root",
                    class: "min-h-screen bg-background text-foreground grid place-items-center px-6",
                    div {
                        id: "aura-onboarding-card",
                        class: "w-full max-w-md rounded-3xl border border-border bg-card px-6 py-8 shadow-sm",
                        div {
                            class: "space-y-2",
                            h1 { class: "text-sm font-semibold uppercase tracking-[0.12em]", "Aura" }
                            h2 { class: "text-2xl font-semibold", "Welcome to Aura" }
                            p {
                                class: "text-sm text-muted-foreground",
                                "Create the local account profile for this browser before entering the app."
                            }
                        }
                        div {
                            class: "mt-6 space-y-4",
                            label {
                                class: "block space-y-2",
                                span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Nickname" }
                                input {
                                    id: "aura-account-name-input",
                                    class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                    value: "{account_name()}",
                                    disabled: creating_account(),
                                    oninput: move |event| {
                                        let value = event.value();
                                        account_name.set(value.clone());
                                        account_error.set(None);
                                        controller.set_account_setup_state(false, value, None);
                                    },
                                }
                            }
                            if let Some(error) = account_error() {
                                p { class: "text-sm text-destructive", "{error}" }
                            }
                            button {
                                id: "aura-onboarding-create-account-button",
                                class: "inline-flex h-10 w-full items-center justify-center rounded-md bg-foreground px-4 text-sm font-medium text-background transition-colors disabled:pointer-events-none disabled:opacity-50",
                                disabled: creating_account() || account_name().trim().is_empty(),
                                onclick: submit_account,
                                if creating_account() {
                                    "Creating Account..."
                                } else {
                                    "Create Account"
                                }
                            }
                            div { class: "flex items-center gap-3 py-1",
                                div { class: "h-px flex-1 bg-border" }
                                span { class: "text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground", "or" }
                                div { class: "h-px flex-1 bg-border" }
                            }
                            label {
                                class: "block space-y-2",
                                span { class: "text-xs font-medium uppercase tracking-[0.08em] text-muted-foreground", "Device Enrollment Code" }
                                input {
                                    id: "aura-account-import-code-input",
                                    class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
                                    value: "{import_code()}",
                                    disabled: importing_code(),
                                    oninput: move |event| {
                                        import_code.set(event.value());
                                        import_error.set(None);
                                    },
                                }
                            }
                            if let Some(error) = import_error() {
                                p { class: "text-sm text-destructive", "{error}" }
                            }
                            button {
                                id: "aura-onboarding-import-device-button",
                                class: "inline-flex h-10 w-full items-center justify-center rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors disabled:pointer-events-none disabled:opacity-50",
                                disabled: importing_code() || import_code().trim().is_empty(),
                                onclick: submit_import,
                                if importing_code() {
                                    "Importing Device..."
                                } else {
                                    "Import Device"
                                }
                            }
                        }
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

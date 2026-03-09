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
        use aura_app::ui::workflows::runtime as runtime_workflows;
        use aura_app::ui::workflows::settings as settings_workflows;
        use aura_app::ui::workflows::time as time_workflows;
        use aura_app::ui::types::InvitationBridgeType;
        use aura_core::{identifiers::AuthorityId, DeviceId};
        use aura_app::ui::contract::{
            ControlId, FieldId, OperationId, OperationInstanceId, OperationSnapshot,
            OperationState, ScreenId, UiReadiness, UiSnapshot,
        };
        use aura_ui::{AuraUiRoot, UiController};
        use dioxus::prelude::*;
        use std::sync::Arc;
        use web_clipboard::WebClipboardAdapter;

        const WEB_STORAGE_PREFIX: &str = "aura_";
        const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";

        fn selected_authority_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}selected_authority")
        }

        fn selected_device_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}selected_device")
        }

        fn pending_device_enrollment_code_key(storage_prefix: &str) -> String {
            format!("{storage_prefix}pending_device_enrollment_code")
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

        fn apply_harness_mode_document_flags() {
            if harness_instance_id().is_none() {
                return;
            }
            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(document) = window.document() else {
                return;
            };
            let Some(root) = document.document_element() else {
                return;
            };
            let _ = root.set_attribute("data-aura-harness-mode", "1");
        }

        fn load_selected_authority(storage_key: &str) -> Option<AuthorityId> {
            let window = web_sys::window()?;
            let storage = window.local_storage().ok().flatten()?;
            let raw = storage.get_item(storage_key).ok().flatten()?;
            raw.parse::<AuthorityId>().ok()
        }

        fn load_selected_device(storage_key: &str) -> Option<DeviceId> {
            let window = web_sys::window()?;
            let storage = window.local_storage().ok().flatten()?;
            let raw = storage.get_item(storage_key).ok().flatten()?;
            raw.parse::<DeviceId>().ok()
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

        fn persist_selected_device(
            storage_key: &str,
            device_id: &DeviceId,
        ) -> Result<(), String> {
            let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
            let storage = window
                .local_storage()
                .map_err(|error| format!("localStorage unavailable: {:?}", error))?
                .ok_or_else(|| "localStorage unavailable".to_string())?;
            storage
                .set_item(storage_key, &device_id.to_string())
                .map_err(|error| format!("failed to persist selected device: {:?}", error))
        }

        fn load_pending_device_enrollment_code(storage_key: &str) -> Option<String> {
            let window = web_sys::window()?;
            let storage = window.local_storage().ok().flatten()?;
            storage.get_item(storage_key).ok().flatten()
        }

        fn persist_pending_device_enrollment_code(
            storage_key: &str,
            code: &str,
        ) -> Result<(), String> {
            let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
            let storage = window
                .local_storage()
                .map_err(|error| format!("localStorage unavailable: {:?}", error))?
                .ok_or_else(|| "localStorage unavailable".to_string())?;
            storage
                .set_item(storage_key, code)
                .map_err(|error| format!("failed to persist pending device enrollment code: {:?}", error))
        }

        fn clear_pending_device_enrollment_code(storage_key: &str) -> Result<(), String> {
            let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
            let storage = window
                .local_storage()
                .map_err(|error| format!("localStorage unavailable: {:?}", error))?
                .ok_or_else(|| "localStorage unavailable".to_string())?;
            storage
                .remove_item(storage_key)
                .map_err(|error| format!("failed to clear pending device enrollment code: {:?}", error))
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
            let device_storage_key = selected_device_key(&storage_prefix);
            let selected_authority = load_selected_authority(&authority_storage_key);
            let selected_device = load_selected_device(&device_storage_key);
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] storage_prefix={storage_prefix};selected_authority={:?};selected_device={:?}",
                    selected_authority, selected_device
                )
                .into(),
            );
            let harness_instance = harness_instance_id();
            let builder = AgentBuilder::web().storage_prefix(&storage_prefix);
            let builder = if let Some(authority_id) = selected_authority {
                builder.authority(authority_id)
            } else {
                builder
            };
            let builder = if let Some(device_id) = selected_device {
                let config = aura_agent::core::AgentConfig {
                    device_id,
                    ..Default::default()
                };
                builder.with_config(config)
            } else {
                builder
            };

            let agent = Arc::new(
                builder
                    .build()
                    .await
                    .map_err(|error| format!("failed to build web runtime: {error}"))?,
            );
            web_sys::console::log_1(
                &format!(
                    "[web-bootstrap] runtime_authority={};runtime_device={}",
                    agent.authority_id(),
                    agent.runtime().device_id()
                )
                .into(),
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
                AppCore::with_runtime(AppConfig::default(), agent.clone().as_runtime_bridge())
                    .map_err(|error| format!("failed to initialize AppCore: {error}"))?,
            ));

            let ceremony_agent = agent.clone();
            let ceremony_app_core = app_core.clone();
            spawn(async move {
                loop {
                    let _ = time_workflows::sleep_ms(&ceremony_app_core, 500).await;
                    if let Err(error) = ceremony_agent.process_ceremony_acceptances().await {
                        web_sys::console::debug_1(
                            &format!("process_ceremony_acceptances error: {error}").into(),
                        );
                    }
                }
            });

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
            controller.set_ui_snapshot_sink(Arc::new(|snapshot| {
                harness_bridge::publish_ui_snapshot(&snapshot);
            }));

            harness_bridge::set_controller(controller.clone());
            if let Err(error) = harness_bridge::install_window_harness_api(controller.clone()) {
                web_sys::console::error_1(
                    &format!("failed to install harness API: {error:?}").into(),
                );
            }

            if account_ready {
                if let Err(error) = settings_workflows::refresh_settings_from_runtime(
                    controller.app_core(),
                )
                .await
                {
                    web_sys::console::warn_1(
                        &format!("failed to seed settings signal during bootstrap: {error}")
                            .into(),
                        );
                }
                match runtime_workflows::require_runtime(controller.app_core()).await {
                    Ok(runtime) => {
                        let runtime_devices = runtime.list_devices().await;
                        match settings_workflows::get_settings(controller.app_core()).await {
                            Ok(settings) => web_sys::console::log_1(
                                &format!(
                                    "[web-bootstrap] settings_seeded runtime_devices={:?};settings_devices={:?}",
                                    runtime_devices
                                        .iter()
                                        .map(|device| device.id.to_string())
                                        .collect::<Vec<_>>(),
                                    settings
                                        .devices
                                        .iter()
                                        .map(|device| device.id.clone())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            ),
                            Err(error) => web_sys::console::warn_1(
                                &format!(
                                    "[web-bootstrap] settings_seeded runtime_devices={:?};settings_error={error}",
                                    runtime_devices
                                        .iter()
                                        .map(|device| device.id.to_string())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            ),
                        }
                    }
                    Err(error) => web_sys::console::warn_1(
                        &format!("[web-bootstrap] failed to inspect runtime devices: {error}")
                            .into(),
                    ),
                }
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
            apply_harness_mode_document_flags();
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
            let mut sync_loop_started = use_signal(|| false);
            let mut account_name = use_signal(String::new);
            let mut account_error = use_signal(|| Option::<String>::None);
            let creating_account = use_signal(|| false);
            let mut import_code = use_signal(String::new);
            let mut import_error = use_signal(|| Option::<String>::None);
            let importing_code = use_signal(|| false);
            let mut auto_import_started = use_signal(|| false);

            let publish_onboarding_snapshot = {
                let controller = controller.clone();
                move || {
                    let base = controller.semantic_model_snapshot();
                    let mut operations = base.operations;
                    if creating_account()
                        && !operations
                            .iter()
                            .any(|operation| operation.id.0 == "account_bootstrap")
                    {
                        operations.push(OperationSnapshot {
                            id: OperationId("account_bootstrap".to_string()),
                            instance_id: OperationInstanceId("account-bootstrap".to_string()),
                            state: OperationState::Submitting,
                        });
                    }
                    controller.set_ui_snapshot(UiSnapshot {
                        screen: ScreenId::Neighborhood,
                        focused_control: Some(ControlId::OnboardingRoot),
                        open_modal: None,
                        readiness: if account_ready() {
                            UiReadiness::Ready
                        } else {
                            UiReadiness::Loading
                        },
                        selections: Vec::new(),
                        lists: Vec::new(),
                        messages: Vec::new(),
                        operations,
                        toasts: base.toasts,
                        runtime_events: base.runtime_events,
                    });
                }
            };

            if account_ready() && !sync_loop_started() {
                sync_loop_started.set(true);
                let app_core = controller.app_core().clone();
                spawn(async move {
                    loop {
                        let runtime = { app_core.read().await.runtime().cloned() };
                        if let Some(runtime) = runtime {
                            let _ = runtime.trigger_discovery().await;
                            let _ = runtime.trigger_sync().await;
                        }
                        let _ = time_workflows::sleep_ms(&app_core, 1_500).await;
                    }
                });
            }

            if account_ready() {
                controller.set_account_setup_state(true, "", None);
                return rsx! {
                    AuraUiRoot {
                        controller: controller.clone(),
                    }
                };
            }

            publish_onboarding_snapshot();

            let run_import: Arc<dyn Fn(String)> = Arc::new({
                let controller = controller.clone();
                let import_error = import_error.clone();
                let importing_code = importing_code.clone();
                let account_ready = account_ready.clone();
                move |code: String| {
                    let mut import_error = import_error.clone();
                    let mut importing_code = importing_code.clone();
                    let mut account_ready = account_ready.clone();
                    if importing_code() {
                        return;
                    }

                    let storage_prefix = active_storage_prefix();
                    let authority_storage_key = selected_authority_key(&storage_prefix);
                    let device_storage_key = selected_device_key(&storage_prefix);
                    let pending_code_storage_key =
                        pending_device_enrollment_code_key(&storage_prefix);
                    importing_code.set(true);
                    import_error.set(None);
                    controller.start_runtime_operation(OperationId::device_enrollment());

                    let controller = controller.clone();
                    spawn(async move {
                        let app_core = controller.app_core().clone();
                        let result = async {
                            let invitation =
                                invitation_workflows::import_invitation_details(&app_core, &code)
                                    .await?;
                            let InvitationBridgeType::DeviceEnrollment {
                                subject_authority,
                                device_id,
                                ..
                            } = invitation.invitation_type.clone()
                            else {
                                return Err(aura_core::AuraError::invalid(
                                    "Code is not a device enrollment invitation",
                                ));
                            };

                            let runtime = runtime_workflows::require_runtime(&app_core).await?;
                            let current_authority = runtime.authority_id();
                            let selected_device = load_selected_device(&device_storage_key);
                            if current_authority != subject_authority
                                || selected_device.as_ref() != Some(&device_id)
                            {
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] staging_reload current_authority={};subject_authority={};selected_device={:?};invited_device={}",
                                        current_authority,
                                        subject_authority,
                                        selected_device,
                                        device_id
                                    )
                                    .into(),
                                );
                                persist_pending_device_enrollment_code(
                                    &pending_code_storage_key,
                                    &code,
                                )
                                .map_err(aura_core::AuraError::agent)?;
                                persist_selected_authority(
                                    &authority_storage_key,
                                    &subject_authority,
                                )
                                .map_err(aura_core::AuraError::agent)?;
                                persist_selected_device(&device_storage_key, &device_id)
                                    .map_err(aura_core::AuraError::agent)?;
                                web_sys::console::log_1(
                                    &format!(
                                        "[web-import-device] staged_reload subject_authority={};device_id={}",
                                        subject_authority, device_id
                                    )
                                    .into(),
                                );
                                reload_page().map_err(aura_core::AuraError::agent)?;
                                return Ok(());
                            }

                            let _ = clear_pending_device_enrollment_code(&pending_code_storage_key);
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepting_on_bound_runtime authority={};selected_device={:?};invited_device={}",
                                    current_authority,
                                    selected_device,
                                    device_id
                                )
                                .into(),
                            );

                            for _ in 0..8 {
                                runtime_workflows::converge_runtime(&runtime).await;
                                if runtime_workflows::ensure_runtime_peer_connectivity(
                                    &runtime,
                                    "device_enrollment_accept",
                                )
                                .await
                                .is_ok()
                                {
                                    break;
                                }
                                time_workflows::sleep_ms(&app_core, 250).await?;
                            }

                            invitation_workflows::accept_device_enrollment_invitation(
                                &app_core,
                                &invitation,
                            )
                            .await?;
                            let runtime_devices_after_accept = runtime.list_devices().await;
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepted runtime_devices={:?}",
                                    runtime_devices_after_accept
                                        .iter()
                                        .map(|device| device.id.to_string())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            );
                            let settings = settings_workflows::get_settings(&app_core).await?;
                            web_sys::console::log_1(
                                &format!(
                                    "[web-import-device] accepted settings_devices={:?}",
                                    settings
                                        .devices
                                        .iter()
                                        .map(|device| device.id.clone())
                                        .collect::<Vec<_>>()
                                )
                                .into(),
                            );
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
                            .await?;
                            reload_page().map_err(aura_core::AuraError::agent)?;
                            Ok(())
                        }
                        .await;

                        match result {
                            Ok(()) => {
                                controller.finish_runtime_operation(
                                    OperationId::device_enrollment(),
                                    OperationState::Succeeded,
                                );
                                controller.info_toast("Device enrollment complete");
                                controller.set_account_setup_state(true, "", None);
                                account_ready.set(true);
                                importing_code.set(false);
                            }
                            Err(error) => {
                                let _ = clear_pending_device_enrollment_code(
                                    &pending_code_storage_key,
                                );
                                controller.finish_runtime_operation(
                                    OperationId::device_enrollment(),
                                    OperationState::Failed,
                                );
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
            });

            let pending_code_storage_key =
                pending_device_enrollment_code_key(&active_storage_prefix());
            if !auto_import_started() {
                if let Some(pending_code) =
                    load_pending_device_enrollment_code(&pending_code_storage_key)
                {
                    if !pending_code.is_empty() {
                        auto_import_started.set(true);
                        import_code.set(pending_code.clone());
                        run_import(pending_code);
                    }
                }
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
                let import_code = import_code.clone();
                let run_import = run_import.clone();
                move |_| {
                    let code = import_code();
                    run_import(code);
                }
            };

            rsx! {
                main {
                    id: ControlId::OnboardingRoot
                        .web_dom_id()
                        .unwrap_or("aura-onboarding-root"),
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
                                    id: FieldId::AccountName
                                        .web_dom_id()
                                        .unwrap_or("aura-account-name-input"),
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
                                id: ControlId::OnboardingCreateAccountButton
                                    .web_dom_id()
                                    .unwrap_or("aura-onboarding-create-account-button"),
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
                                    id: FieldId::DeviceImportCode
                                        .web_dom_id()
                                        .unwrap_or("aura-account-import-code-input"),
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
                                id: ControlId::OnboardingImportDeviceButton
                                    .web_dom_id()
                                    .unwrap_or("aura-onboarding-import-device-button"),
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

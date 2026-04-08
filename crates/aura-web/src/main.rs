//! Aura web application entry point for WASM targets.
//!
//! Initializes the Dioxus-based web UI with the AppCore, clipboard adapter,
//! and harness bridge for browser-based deployment and testing.

#![allow(missing_docs)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod bootstrap_storage;
        mod browser_promises;
        mod error;
        mod harness;
        mod harness_bridge;
        mod shell;
        mod shell_host;
        mod task_owner;
        mod web_clipboard;
        mod workflows;

        use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
        use shell::{apply_harness_mode_document_flags, App};

        pub(crate) use shell::{
            active_storage_prefix, clear_pending_device_enrollment_code, clear_storage_key,
            bootstrap_broker_url, device_enrollment_bootstrap_name,
            dual_demo_web_enabled, harness_instance_id, harness_mode_enabled,
            load_pending_account_bootstrap, load_pending_device_enrollment_code,
            load_selected_runtime_identity, logged_optional, pending_account_bootstrap_key,
            pending_device_enrollment_code_key, persist_pending_device_enrollment_code,
            persist_selected_runtime_identity, selected_runtime_identity_key,
            stage_initial_web_account_bootstrap, stage_runtime_bound_web_account_bootstrap,
            submit_runtime_bootstrap_handoff, submit_runtime_bootstrap_handoff_accepted,
        };

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
    } else {
        fn main() {
            eprintln!("aura-web is a wasm32 frontend. Build with target wasm32-unknown-unknown.");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn web_harness_ui_state_observation_fails_closed_without_published_snapshot() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        assert!(source.contains("UI_PUBLICATION_STATE_KEY"));
        assert!(source.contains("semantic_snapshot_not_published"));
        assert!(!source.contains("return live_json;"));
    }

    #[test]
    fn web_harness_publication_failures_are_structurally_observable() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        assert!(source.contains("RENDER_HEARTBEAT_PUBLICATION_STATE_KEY"));
        assert!(source.contains("\"degraded\""));
        assert!(source.contains("\"unavailable\""));
        assert!(source.contains("cache_publish_failed"));
        assert!(source.contains("heartbeat_publish_failed"));
    }

    #[test]
    fn web_ui_publication_keeps_console_json_fallback_even_with_driver_push() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        assert!(
            source.contains("[aura-ui-json]{json}")
                && source.contains("if binding_mode == PublicationBindingMode::WindowCacheOnly {"),
            "ui_state publication must keep the console JSON fallback even when driver_push is available so a missed Playwright callback cannot strand semantic observation during generation handoff"
        );
    }

    #[test]
    fn web_semantic_submit_publication_keeps_console_json_fallback() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        assert!(
            source.contains("[aura-semantic-submit-json]{json}")
                && source.contains("PUSH_SEMANTIC_SUBMIT_STATE_KEY"),
            "semantic-submit publication must keep a structured console JSON fallback alongside the scheduled Playwright callback so queue readiness survives callback delivery gaps during generation rebinding"
        );
    }

    #[test]
    fn web_background_sync_exit_is_structurally_visible() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        assert!(source.contains("pub(crate) fn spawn_browser_maintenance_loop<"));
        assert!(source.contains("fn spawn_generation_maintenance_supervisor("));
        assert!(source.contains("_controller.runtime_error_toast(_pause_message);"));
        assert!(source.contains("run_background_sync_pass(tick_app_core).await;"));
        assert!(source.contains("\"Browser maintenance paused; refresh to resume\""));
        assert!(source.contains("\"WEB_GENERATION_MAINTENANCE_SLEEP_FAILED\""));
    }

    #[test]
    fn web_browser_maintenance_yields_before_first_tick() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        let helper_start = source
            .find("pub(crate) fn spawn_browser_maintenance_loop<")
            .unwrap_or_else(|| panic!("missing spawn_browser_maintenance_loop"));
        let helper_end = source[helper_start..]
            .find("async fn run_background_sync_pass(")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing run_background_sync_pass"));
        let helper = &source[helper_start..helper_end];
        let sleep_index = helper
            .find("browser_sleep_ms(")
            .unwrap_or_else(|| panic!("missing browser_sleep_ms call"));
        let tick_index = helper
            .find("tick().await;")
            .unwrap_or_else(|| panic!("missing tick call"));
        assert!(
            sleep_index < tick_index,
            "browser maintenance loops must yield once before the first background tick so bootstrap-handoff interactivity keeps priority over sync/transport work"
        );
    }

    #[test]
    fn web_background_sync_is_cooperative_between_heavy_steps() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        let helper_start = source
            .find("async fn run_background_sync_pass(")
            .unwrap_or_else(|| panic!("missing run_background_sync_pass"));
        let helper_end = source[helper_start..]
            .find("fn spawn_generation_maintenance_supervisor(")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing spawn_generation_maintenance_supervisor"));
        let helper = &source[helper_start..helper_end];

        assert!(source.contains("async fn yield_browser_maintenance_step("));
        assert!(helper.contains("\"background sync before trigger_discovery\""));
        assert!(helper.contains("\"background sync before process_ceremony_messages_before_sync\""));
        assert!(helper.contains("\"background sync before trigger_sync\""));
        assert!(helper.contains("\"background sync before process_ceremony_messages_after_sync\""));
        assert!(helper.contains("\"background sync before refresh_account\""));
        assert!(helper.contains("\"background sync before refresh_discovered_peers\""));
        assert!(helper.contains("\"WEB_BACKGROUND_SYNC_YIELD_FAILED\""));
    }

    #[test]
    fn web_generation_maintenance_is_owned_by_browser_loop() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        assert!(source.contains("fn spawn_generation_maintenance_supervisor("));
        assert!(source.contains("if let Some(agent) = agent.clone() {"));
        assert!(source.contains("if browser_harness_mode {"));
        assert!(source.contains("\"transport_tick_start\""));
        assert!(source.contains("\"transport_tick_polled\""));
        assert!(source.contains("\"Browser maintenance paused; refresh to resume\""));
        assert!(source.contains("\"WEB_GENERATION_MAINTENANCE_SLEEP_FAILED\""));
    }

    #[test]
    fn web_semantic_snapshot_publication_is_centralized() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let main_path = repo_root.join("crates/aura-web/src/main.rs");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let main_source = std::fs::read_to_string(&main_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_path.display()));
        let shell_host_source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });
        let publication_source =
            std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", publication_path.display())
            });
        let production_main = main_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_else(|| panic!("missing production main section"));

        assert!(publication_source.contains("pub(crate) fn publish_semantic_controller_snapshot"));
        assert!(publication_source.contains("controller.publish_ui_snapshot(snapshot.clone())"));
        assert!(shell_host_source
            .contains("harness_bridge::publish_semantic_controller_snapshot(controller.clone())"));
        assert!(!production_main
            .contains("harness_bridge::publish_semantic_controller_snapshot(controller.clone())"));
        assert!(!production_main.contains("harness_bridge::publish_ui_snapshot(&final_snapshot)"));
        assert!(!production_main.contains("harness_bridge::publish_ui_snapshot(&initial_snapshot)"));
    }

    #[test]
    fn web_generation_reset_clears_controller_and_publication_snapshot_dedup() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let bridge_source = std::fs::read_to_string(&bridge_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", bridge_path.display()));

        let set_generation_start = bridge_source
            .find("pub fn set_active_generation(generation_id: u64) {")
            .unwrap_or_else(|| panic!("missing set_active_generation"));
        let set_generation_end = bridge_source[set_generation_start..]
            .find("pub fn set_bootstrap_transition_detail(")
            .map(|offset| set_generation_start + offset)
            .unwrap_or_else(|| panic!("missing set_bootstrap_transition_detail"));
        let set_generation = &bridge_source[set_generation_start..set_generation_end];
        assert!(set_generation.contains("controller.reset_published_ui_snapshot();"));
        assert!(set_generation.contains("reset_published_ui_snapshot_dedup();"));

        let set_controller_start = bridge_source
            .find("pub fn set_controller(controller: Arc<UiController>) {")
            .unwrap_or_else(|| panic!("missing set_controller"));
        let set_controller_end = bridge_source[set_controller_start..]
            .find("pub fn clear_controller(reason: &str) {")
            .map(|offset| set_controller_start + offset)
            .unwrap_or_else(|| panic!("missing clear_controller"));
        let set_controller = &bridge_source[set_controller_start..set_controller_end];
        assert!(set_controller.contains("controller.reset_published_ui_snapshot();"));
        assert!(set_controller.contains("reset_published_ui_snapshot_dedup();"));
    }

    #[test]
    fn web_bootstrap_sets_account_gate_before_initial_harness_publication() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let runtime_branch_start = source
            .find("let controller = Arc::new(UiController::with_authority_switcher(")
            .unwrap_or_else(|| panic!("missing runtime bootstrap controller"));
        let runtime_branch = &source[runtime_branch_start..];
        let runtime_gate_index = runtime_branch
            .find("controller.set_account_setup_state(account_ready, \"\", None);")
            .unwrap_or_else(|| panic!("missing runtime bootstrap account gate"));
        let runtime_install_index = runtime_branch
            .find("install_harness_instrumentation(")
            .unwrap_or_else(|| panic!("missing runtime bootstrap harness install"));
        assert!(
            runtime_gate_index < runtime_install_index,
            "runtime bootstrap must set the account gate before initial harness publication"
        );

        let shell_branch_start = source
            .find("let controller = Arc::new(UiController::new(app_core, clipboard));")
            .unwrap_or_else(|| panic!("missing shell bootstrap controller"));
        let shell_branch_end = source[shell_branch_start..]
            .find("let waiting_event = BootstrapEvent::new(")
            .map(|offset| shell_branch_start + offset)
            .unwrap_or_else(|| panic!("missing shell waiting event"));
        let shell_branch = &source[shell_branch_start..shell_branch_end];
        let shell_gate_index = shell_branch
            .find("controller.set_account_setup_state(false, \"\", None);")
            .unwrap_or_else(|| panic!("missing shell bootstrap account gate"));
        let shell_install_index = shell_branch
            .find("install_harness_instrumentation(")
            .unwrap_or_else(|| panic!("missing shell bootstrap harness install"));
        assert!(
            shell_gate_index < shell_install_index,
            "shell bootstrap must publish onboarding state only after applying the account gate"
        );
    }

    #[test]
    fn web_harness_selection_helpers_use_canonical_snapshot_selections_only() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let selection_path = repo_root.join("crates/aura-web/src/harness/channel_selection.rs");
        let source = std::fs::read_to_string(&selection_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", selection_path.display()));

        assert!(source.contains("pub(crate) struct WeakChannelSelection(ChannelId);"));

        let channel_start = source
            .find("pub(crate) fn selected_channel_id(")
            .unwrap_or_else(|| panic!("missing selected_channel_id"));
        let channel_end = source[channel_start..]
            .find("pub(crate) async fn selected_channel_binding(")
            .map(|offset| channel_start + offset)
            .unwrap_or(source.len());
        let channel_block = &source[channel_start..channel_end];
        assert!(channel_block.contains(".selected_channel_id()"));
        assert!(!channel_block.contains(".selected_item_id(ListId::Channels)"));

        let device_start = source
            .find("pub(crate) fn selected_device_id(controller: &UiController)")
            .unwrap_or_else(|| panic!("missing selected_device_id"));
        let device_end = source[device_start..]
            .find("pub(crate) fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .map(|offset| device_start + offset)
            .unwrap_or(source.len());
        let device_block = &source[device_start..device_end];
        assert!(device_block.contains(".selected_item_id(ListId::Devices)"));
        assert!(!device_block.contains("list.items.len() == 1"));

        let authority_start = source
            .find("pub(crate) fn selected_authority_id(controller: &UiController) -> Option<String> {")
            .unwrap_or_else(|| panic!("missing selected_authority_id"));
        let authority_end = source[authority_start..]
            .find("}")
            .map(|offset| authority_start + offset)
            .unwrap_or(source.len());
        let authority_block = &source[authority_start..authority_end];
        assert!(authority_block.contains(".selected_authority_id()"));
        assert!(!authority_block.contains(".selected_item_id(ListId::Authorities)"));
    }

    #[test]
    fn harness_bridge_selection_helpers_use_canonical_snapshot_selections_only() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let selection_path = repo_root.join("crates/aura-web/src/harness/channel_selection.rs");
        let source = std::fs::read_to_string(&selection_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", selection_path.display()));

        assert!(source.contains("pub(crate) struct WeakChannelSelection(ChannelId);"));
        assert!(source.contains(".selected_channel_id()"));
        assert!(source.contains(".selected_item_id(ListId::Devices)"));
        assert!(!source.contains(".selected_item_id(ListId::Channels)"));
    }

    #[test]
    fn web_semantic_command_execution_uses_declared_submission_helpers() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let commands_path = repo_root.join("crates/aura-web/src/harness/commands.rs");
        let source = std::fs::read_to_string(&commands_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", commands_path.display()));
        let submit_semantic_command_marker =
            ["pub(crate) async fn submit_", "semantic_command("].concat();

        let start = source
            .find("async fn execute_semantic_intent(")
            .unwrap_or_else(|| panic!("missing execute_semantic_intent"));
        let end = source[start..]
            .find(&submit_semantic_command_marker)
            .map(|offset| start + offset)
            .unwrap_or_else(|| panic!("missing submit_semantic_command marker"));
        let body = &source[start..end];

        for forbidden in [
            "SemanticCommandResponse::accepted_without_value()",
            "begin_exact_handoff_operation(",
            "begin_exact_ui_operation(",
            "semantic_response_with_handle(",
            "semantic_unit_result_with_handle(",
            "semantic_channel_result(",
            "semantic_channel_result_with_handle(",
        ] {
            assert!(
                !body.contains(forbidden),
                "execute_semantic_intent should use declared submission helpers instead of `{forbidden}`"
            );
        }
    }

    #[test]
    fn web_harness_channel_join_selects_and_send_prefers_authoritative_binding() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let commands_path = repo_root.join("crates/aura-web/src/harness/commands.rs");
        let source = std::fs::read_to_string(&commands_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", commands_path.display()));
        let select_created = [
            "controller.select_channel_by_id(",
            "&created.channel_id.to_string());",
        ]
        .concat();
        let set_screen_call = ["controller.set_", "screen(screen);"].concat();
        let select_channel = ["controller.select_channel_by_id(", "channel_id);"].concat();
        let select_binding = ["controller.select_channel_by_id(", "&binding.channel_id);"].concat();

        let create_start = source
            .find("RoutedSemanticIntent::CreateChannel { channel_name } => {")
            .unwrap_or_else(|| panic!("missing create channel intent"));
        let create_end = source[create_start..]
            .find("RoutedSemanticIntent::StartDeviceEnrollment {")
            .map(|offset| create_start + offset)
            .unwrap_or_else(|| panic!("missing create channel terminator"));
        let create_block = &source[create_start..create_end];
        assert!(create_block.contains("create_channel_with_authoritative_binding("));
        assert!(!create_block.contains("select_channel_by_id_after_row_visible("));
        assert!(!create_block.contains(&select_created));

        let open_start = source
            .find("RoutedSemanticIntent::OpenScreen { screen, channel_id } => {")
            .unwrap_or_else(|| panic!("missing open screen intent"));
        let open_end = source[open_start..]
            .find("RoutedSemanticIntent::CreateAccount { account_name } => {")
            .map(|offset| open_start + offset)
            .unwrap_or_else(|| panic!("missing open screen terminator"));
        let open_block = &source[open_start..open_end];
        assert!(open_block.contains(&set_screen_call));
        assert!(open_block.contains(&select_channel));
        assert!(open_block.contains("refresh_account(controller.app_core())"));

        let join_start = source
            .find("RoutedSemanticIntent::JoinChannel { channel_name } => {")
            .unwrap_or_else(|| panic!("missing join channel intent"));
        let join_end = source[join_start..]
            .find("RoutedSemanticIntent::InviteActorToChannel {")
            .map(|offset| join_start + offset)
            .unwrap_or_else(|| panic!("missing join channel terminator"));
        let join_block = &source[join_start..join_end];
        assert!(join_block.contains("join_channel_by_name_with_binding_terminal_status("));
        assert!(join_block.contains(&select_binding));

        let send_start = source
            .find("IntentAction::SendChatMessage {")
            .unwrap_or_else(|| panic!("missing send chat route"));
        let send_end = source[send_start..]
            .find("}\n    }\n}")
            .map(|offset| send_start + offset)
            .unwrap_or(source.len());
        let send_block = &source[send_start..send_end];
        assert!(send_block.contains("selected_channel_binding(controller)"));
        assert!(send_block.contains("Some(channel_id) => channel_id"));
    }

    #[test]
    fn web_harness_install_delegates_semantic_submission_to_commands_module() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let install_path = repo_root.join("crates/aura-web/src/harness/install.rs");
        let source = std::fs::read_to_string(&install_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", install_path.display()));

        assert!(source.contains("page_owned_queue::install(window)"));
        assert!(
            source.contains("commands::BrowserSemanticBridgeRequest::from_json(&request_json)?")
        );
        assert!(!source.contains("include_str!(\"page_owned_mutation_queues.js\")"));
        assert!(!source.contains("Function::new_no_args("));
        assert!(!source.contains("route_semantic_intent("));
        assert!(!source.contains("execute_semantic_intent("));
    }

    #[test]
    fn web_harness_window_contract_centralizes_browser_globals() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let queue_path = repo_root.join("crates/aura-web/src/harness/page_owned_queue.rs");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let queue_source = std::fs::read_to_string(&queue_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", queue_path.display()));
        let publication_source =
            std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", publication_path.display())
            });

        for source in [&queue_source, &publication_source] {
            assert!(source.contains("use crate::harness::window_contract::"));
            assert!(!source.contains("Reflect::get(window.as_ref()"));
            assert!(!source.contains("Reflect::set(window.as_ref()"));
        }
    }

    #[test]
    fn web_bootstrap_handoff_waits_for_completion() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let helper_start = source
            .find("pub(crate) async fn complete_handoff")
            .unwrap_or_else(|| panic!("missing complete_handoff"));
        let helper_end = source[helper_start..]
            .find("fn install_harness_instrumentation")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing install_harness_instrumentation"));
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("bootstrap_generation(epoch).await"));
        assert!(helper.contains("let previous_bootstrap = {"));
        assert!(helper.contains("slot.take()"));
        assert!(helper.contains("harness_bridge::clear_controller("));
        assert!(
            helper.contains("shutdown_previous_bootstrap_generation(previous_bootstrap).await;")
        );
        assert!(helper.contains("harness_bridge::wait_for_generation_ready(generation_id)"));
        assert!(helper.contains("set_browser_shell_phase(BrowserShellPhase::Ready)"));
        assert!(helper.contains("Err(error)"));
        assert!(!helper.contains("spawn_local(async move"));
    }

    #[test]
    fn web_rebootstrap_shuts_down_previous_runtime_generation() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        let helper_start = source
            .find("async fn shutdown_previous_bootstrap_generation(")
            .unwrap_or_else(|| panic!("missing shutdown_previous_bootstrap_generation"));
        let helper_end = source[helper_start..]
            .find("\nasync fn hydrate_existing_runtime_account_projection")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing hydrate_existing_runtime_account_projection"));
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("let app_core = controller.app_core().clone();"));
        assert!(helper.contains("AppCore::detach_runtime(&app_core).await;"));
        assert!(helper.contains("drop(controller);"));
        assert!(helper.contains("Arc::try_unwrap(agent)"));
        assert!(helper.contains("agent.shutdown(&effect_context).await"));
        assert!(helper.contains("for attempt in 1..=8 {"));
        assert!(helper.contains("browser_sleep_ms("));
        assert!(helper.contains("WEB_PREVIOUS_GENERATION_RUNTIME_SHUTDOWN_SKIPPED"));
    }

    #[test]
    fn web_harness_immediate_bootstrap_commands_use_accepted_handoff_submission() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let commands_path = repo_root.join("crates/aura-web/src/harness/commands.rs");
        let source = std::fs::read_to_string(&commands_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", commands_path.display()));

        assert!(source.contains("fn schedule_immediate_bootstrap_handoff("));
        assert!(source.contains("schedule_browser_task_next_tick(move ||"));
        assert!(source.contains("submit_runtime_bootstrap_handoff_accepted(handoff)"));
        assert!(source.contains("create_account_handoff_done"));
    }

    #[test]
    fn web_page_owned_queue_drains_pending_seed_payloads_during_run() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let queue_path = repo_root.join("crates/aura-web/src/harness/page_owned_queue.rs");
        let source = std::fs::read_to_string(&queue_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", queue_path.display()));

        assert!(source.contains("fn drain_pending_seed_queues() {"));
        assert!(source.contains("fn run_semantic_queue() {\n    drain_pending_seed_queues();"));
        assert!(source.contains("fn run_runtime_stage_queue() {\n    drain_pending_seed_queues();"));
        assert!(source.contains("drain_seed_queues(&window)"));
    }

    #[test]
    fn web_page_owned_queue_installs_dom_ingress_fallback_for_semantic_enqueue() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let queue_path = repo_root.join("crates/aura-web/src/harness/page_owned_queue.rs");
        let source = std::fs::read_to_string(&queue_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", queue_path.display()));

        assert!(source.contains("install_semantic_ingress(window)?;"));
        assert!(source.contains("textarea.set_id(SEMANTIC_QUEUE_INGRESS_ID);"));
        assert!(source.contains("add_event_listener_with_callback(\"input\""));
        assert!(source.contains("accept_semantic_payload_json(&payload_json, \"live_enqueue\")"));
        assert!(source.contains("accept_semantic_payload_json(&payload_json, \"dom_ingress\")"));
    }

    #[test]
    fn web_generation_maintenance_serializes_browser_upkeep() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        let start = source
            .find("pub(crate) fn spawn_generation_maintenance_loops(")
            .unwrap_or_else(|| panic!("missing spawn_generation_maintenance_loops"));
        let body = &source[start..];

        assert!(source.contains("fn spawn_generation_maintenance_supervisor("));
        assert!(body.contains("spawn_generation_maintenance_supervisor(&owner"));
        assert!(!body.contains("spawn_background_sync_loop(&owner"));
        assert!(!body.contains("spawn_harness_transport_poll_loop("));
        assert!(!body.contains("spawn_ceremony_acceptance_loop("));
    }

    #[test]
    fn web_generation_maintenance_debug_events_do_not_fetch_per_tick() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        let start = source
            .find("#[cfg(target_arch = \"wasm32\")]\nfn emit_browser_harness_debug_event(")
            .unwrap_or_else(|| panic!("missing emit_browser_harness_debug_event"));
        let body = &source[start..];

        assert!(
            !body.contains("fetch_with_str(&url)"),
            "browser maintenance diagnostics must not issue per-tick fetch traffic from the wasm page; that diagnostic side channel can starve Playwright actionability on preserved-profile restarts"
        );
    }

    #[test]
    fn web_generation_maintenance_transport_poll_budget_stays_coarse() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        assert!(
            source.contains("const HARNESS_TRANSPORT_POLL_INTERVAL_MS: u64 = 1_000;"),
            "browser steady-state harness transport polling must stay on a coarse cadence; a 100 ms browser poll loop can starve Playwright actionability on preserved-profile runs"
        );
    }

    #[test]
    fn web_harness_background_sync_grants_bootstrap_interactivity_window() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        assert!(
            source.contains("const HARNESS_BACKGROUND_SYNC_INTERVAL_MS: u64 = 10_000;")
                && source.contains("const HARNESS_BACKGROUND_SYNC_START_DELAY_MS: u64 = 15_000;")
                && source.contains("tick_count >= background_sync_start_delay_ticks"),
            "harness-mode browser background sync must leave an initial interactivity window after bootstrap handoff instead of starting full sync work on the first maintenance tick"
        );
    }

    #[test]
    fn web_driver_push_callbacks_are_scheduled_off_page_critical_paths() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let queue_path = repo_root.join("crates/aura-web/src/harness/page_owned_queue.rs");
        let publication_source =
            std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", publication_path.display())
            });
        let queue_source = std::fs::read_to_string(&queue_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", queue_path.display()));

        assert!(
            publication_source.contains("pub(crate) fn schedule_window_callback_push(")
                && publication_source.contains("Function::new_with_args(")
                && publication_source.contains(
                    "set_timeout_with_callback_and_timeout_and_arguments_3("
                ),
            "browser publication should schedule driver callback pushes through a page-owned JS callback so semantic/render publication does not depend on a Rust generation-owned timer closure"
        );
        assert!(
            publication_source.contains(".function(DRIVER_PUSH_UI_STATE_KEY)")
                && publication_source.contains("schedule_window_callback_push(\n            window,\n            DRIVER_PUSH_UI_STATE_KEY,")
                && publication_source.contains("schedule_window_callback_push(\n        window,\n        DRIVER_PUSH_RENDER_HEARTBEAT_KEY,")
                && publication_source.contains("schedule_window_callback_push(\n        window,\n        PUSH_SEMANTIC_SUBMIT_STATE_KEY,"),
            "browser publication should publish authoritative ui_state cache/json inline, while all Playwright callback pushes stay scheduled off the browser critical path"
        );
        assert!(
            queue_source.contains("schedule_window_callback_push(\n            window_contract.raw_window(),\n            PUSH_SEMANTIC_SUBMIT_STATE_KEY,")
                && queue_source.contains("let _ = function.call1(window_contract.raw_window().as_ref(), &payload);"),
            "page-owned queue should keep semantic submit state pushes best-effort and scheduled, while terminal semantic/runtime result delivery remains immediate for the driver request/response contract"
        );
    }

    #[test]
    fn web_bootstrap_generation_uses_typed_phase_tracker() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_host_path = repo_root.join("crates/aura-web/src/shell_host.rs");
        let source = std::fs::read_to_string(&shell_host_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", shell_host_path.display())
        });

        assert!(source.contains("enum BootstrapPhase {"));
        assert!(source.contains("struct BootstrapPhaseTracker {"));
        assert!(source.contains("WEB_BOOTSTRAP_PHASE_ORDER_INVALID"));
        assert!(source.contains("bootstrap phase "));

        let bootstrap_start = source
            .find("async fn bootstrap_generation(generation_id: u64) -> Result<BootstrapState, WebUiError> {")
            .unwrap_or_else(|| panic!("missing bootstrap_generation"));
        let bootstrap_end = source[bootstrap_start..]
            .find("\n#[cfg(test)]")
            .map(|offset| bootstrap_start + offset)
            .unwrap_or(source.len());
        let bootstrap_block = &source[bootstrap_start..bootstrap_end];

        let init_index = bootstrap_block
            .find("BootstrapPhaseTracker::new(generation_id)")
            .unwrap_or_else(|| panic!("missing phase tracker initialization"));
        let resolve_index = bootstrap_block
            .find("phase.advance_to(BootstrapPhase::ResolveRuntimeIdentity)?;")
            .unwrap_or_else(|| panic!("missing resolve phase transition"));
        let install_index = bootstrap_block
            .find("phase.advance_to(BootstrapPhase::InstallHarness)?;")
            .unwrap_or_else(|| panic!("missing install phase transition"));
        let publish_index = bootstrap_block
            .find("phase.advance_to(BootstrapPhase::PublishFinalSnapshot)?;")
            .unwrap_or_else(|| panic!("missing publish phase transition"));
        let ready_index = bootstrap_block
            .find("phase.advance_to(BootstrapPhase::Ready)?;")
            .unwrap_or_else(|| panic!("missing ready phase transition"));

        assert!(init_index < resolve_index);
        assert!(resolve_index < install_index);
        assert!(install_index < publish_index);
        assert!(publish_index < ready_index);
    }

    #[test]
    fn semantic_ui_snapshot_publication_precedes_render_heartbeat_scheduling() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        let publish_start = source
            .find("pub(crate) fn publish_ui_snapshot(snapshot: &UiSnapshot) {")
            .unwrap_or_else(|| panic!("missing publish_ui_snapshot"));
        let publish_block = &source[publish_start..];
        let semantic_publish_index = publish_block
            .find("publish_ui_snapshot_now(&window, value, json, screen, open_modal, operation_count)")
            .unwrap_or_else(|| panic!("missing semantic snapshot publication"));
        let animation_frame_index = publish_block
            .find("window.request_animation_frame(callback_fn)")
            .unwrap_or_else(|| panic!("missing render heartbeat scheduling"));

        assert!(
            semantic_publish_index < animation_frame_index,
            "semantic snapshot publication must happen before render-heartbeat scheduling"
        );
    }

    #[test]
    fn generation_ready_tracks_snapshot_publication_not_domain_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let publication_path = repo_root.join("crates/aura-web/src/harness/publication.rs");
        let source = std::fs::read_to_string(&publication_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", publication_path.display())
        });

        let publish_start = source
            .find("pub(crate) fn publish_ui_snapshot(snapshot: &UiSnapshot) {")
            .unwrap_or_else(|| panic!("missing publish_ui_snapshot"));
        let publish_block = &source[publish_start..];

        assert!(
            publish_block.contains("mark_generation_ready(generation_id);"),
            "published snapshots must mark the active generation ready through the canonical publication path",
        );
        assert!(
            !publish_block.contains("snapshot.readiness == UiReadiness::Ready"),
            "generation-ready publication must not be gated on domain UiReadiness::Ready",
        );
    }

    #[test]
    fn web_runtime_account_paths_persist_browser_account_config() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let app_path = repo_root.join("crates/aura-web/src/shell/app.rs");
        let source = std::fs::read_to_string(&app_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", app_path.display()));

        assert!(
            source.contains("workflows::accept_device_enrollment_import("),
            "device enrollment import should route through the shared aura-web workflow helper"
        );
        assert!(
            source.contains("workflows::stage_account_creation(controller.app_core(), &nickname)"),
            "onboarding account creation should route through the shared aura-web workflow helper"
        );
    }

    #[test]
    fn web_onboarding_lists_bootstrap_candidates_separately_from_join_flow() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let app_path = repo_root.join("crates/aura-web/src/shell/app.rs");
        let source = std::fs::read_to_string(&app_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", app_path.display()));

        assert!(
            source.contains("Local devices available for enrollment"),
            "web onboarding should surface bootstrap candidates separately"
        );
        assert!(
            source.contains("Join an existing account"),
            "web onboarding should keep the existing device-enrollment import flow"
        );
    }
}

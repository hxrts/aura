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
            device_enrollment_bootstrap_name, harness_instance_id, harness_mode_enabled,
            load_pending_account_bootstrap, load_pending_device_enrollment_code,
            load_selected_runtime_identity, logged_optional, pending_account_bootstrap_key,
            pending_device_enrollment_code_key, persist_pending_device_enrollment_code,
            persist_selected_runtime_identity, selected_runtime_identity_key,
            stage_initial_web_account_bootstrap, stage_runtime_bound_web_account_bootstrap,
            submit_runtime_bootstrap_handoff,
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

        assert!(source.contains("__AURA_UI_PUBLICATION_STATE__"));
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

        assert!(source.contains("__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__"));
        assert!(source.contains("\"degraded\""));
        assert!(source.contains("\"unavailable\""));
        assert!(source.contains("driver_push_failed"));
    }

    #[test]
    fn web_background_sync_exit_is_structurally_visible() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        assert!(source.contains("pub(crate) fn spawn_browser_maintenance_loop<"));
        assert!(source.contains("controller.runtime_error_toast(pause_message);"));

        let helper_start = source
            .find("pub(crate) fn spawn_background_sync_loop")
            .unwrap_or_else(|| panic!("missing spawn_background_sync_loop"));
        let helper = &source[helper_start..];

        assert!(helper.contains("spawn_browser_maintenance_loop("));
        assert!(helper.contains("\"Background sync paused; refresh to resume\""));
        assert!(helper.contains("\"WEB_BACKGROUND_SYNC_SLEEP_FAILED\""));
    }

    #[test]
    fn web_ceremony_acceptance_exit_is_structurally_visible() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let maintenance_path = repo_root.join("crates/aura-web/src/shell/maintenance.rs");
        let source = std::fs::read_to_string(&maintenance_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", maintenance_path.display())
        });

        let helper_start = source
            .find("fn spawn_ceremony_acceptance_loop")
            .unwrap_or_else(|| panic!("missing spawn_ceremony_acceptance_loop"));
        let helper = &source[helper_start..];

        assert!(helper.contains("spawn_browser_maintenance_loop("));
        assert!(helper.contains("\"Ceremony acceptance paused; refresh to resume\""));
        assert!(helper.contains("\"WEB_CEREMONY_ACCEPTANCE_SLEEP_FAILED\""));
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

        let start = source
            .find("async fn execute_semantic_intent(")
            .unwrap_or_else(|| panic!("missing execute_semantic_intent"));
        let end = source[start..]
            .find("pub(crate) async fn submit_semantic_command(")
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
    fn web_harness_install_delegates_semantic_submission_to_commands_module() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let install_path = repo_root.join("crates/aura-web/src/harness/install.rs");
        let source = std::fs::read_to_string(&install_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", install_path.display()));

        assert!(source.contains("page_owned_queue::install(window)"));
        assert!(source.contains("commands::BrowserSemanticBridgeRequest::from_json(&request_json)?"));
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
        let publication_source = std::fs::read_to_string(&publication_path).unwrap_or_else(
            |error| panic!("failed to read {}: {error}", publication_path.display()),
        );

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
        assert!(helper.contains("committed_bootstrap.set(None);"));
        assert!(helper.contains("harness_bridge::clear_controller("));
        assert!(helper.contains("harness_bridge::wait_for_generation_ready(generation_id)"));
        assert!(helper.contains("set_browser_shell_phase(BrowserShellPhase::Ready)"));
        assert!(helper.contains("Err(error)"));
        assert!(!helper.contains("spawn_local(async move"));
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
}

//! Phase 1 regression tests.

#![allow(missing_docs)]

use std::fs;
use std::process::Command;

use aura_harness::config::{
    InstanceConfig, InstanceMode, RunConfig, RunSection, RuntimeSubstrate, ScreenSource,
};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn two_local_instances_are_controllable() {
    let root = std::env::temp_dir().join("aura-harness-phase1-regression");
    let _ = fs::create_dir_all(&root);

    let run_config = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "phase1-regression".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(2),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: RuntimeSubstrate::default(),
        },
        instances: vec![
            instance("alice", root.join("alice"), "127.0.0.1:42001"),
            instance("bob", root.join("bob"), "127.0.0.1:42002"),
        ],
    };

    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut tool_api = ToolApi::new(coordinator);

    if let Err(error) = tool_api.start_all() {
        panic!("start_all failed: {error}");
    }

    assert_ok(tool_api.handle_request(ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "alice-msg\n".to_string(),
    }));
    assert_ok(tool_api.handle_request(ToolRequest::SendKeys {
        instance_id: "bob".to_string(),
        keys: "bob-msg\n".to_string(),
    }));

    assert_ok(tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "alice-msg".to_string(),
        selector: None,
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }));
    assert_ok(tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "bob".to_string(),
        pattern: "bob-msg".to_string(),
        selector: None,
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }));

    if let Err(error) = tool_api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let events = tool_api.event_snapshot();
    assert!(events
        .iter()
        .any(|event| event.instance_id.as_deref() == Some("alice")));
    assert!(events
        .iter()
        .any(|event| event.instance_id.as_deref() == Some("bob")));
}

#[test]
fn invalid_toml_fails_before_process_launch() {
    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir creation failed: {error}"),
    };
    let bad_file = temp.path().join("bad.toml");
    if let Err(error) = fs::write(&bad_file, "schema_version = 1\n[run]\nname = \"x\"\n") {
        panic!("failed to write bad file: {error}");
    }

    let error = match aura_harness::load_and_validate_run_config(&bad_file) {
        Ok(_) => panic!("invalid config should fail"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("failed to parse run config TOML"),
        "unexpected error: {error}"
    );
}

#[test]
fn run_command_emits_artifacts_bundle() {
    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir creation failed: {error}"),
    };

    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");
    let artifact_root = temp.path().join("artifacts");

    let run_toml = format!(
        r#"schema_version = 1

[run]
name = "phase1-artifacts"
pty_rows = 40
pty_cols = 120
artifact_dir = "{}"

[[instances]]
id = "alice"
mode = "local"
data_dir = "{}"
bind_address = "127.0.0.1:43001"
command = "bash"
args = ["-lc", "cat"]
"#,
        artifact_root.display(),
        temp.path().join("alice-data").display()
    );
    if let Err(error) = fs::write(&config_path, run_toml) {
        panic!("failed writing run config: {error}");
    }

    let scenario_toml = r#"id = "phase1-artifacts"
goal = "validate artifact generation"

[[steps]]
id = "seed"
action = "launch_actors"
"#;
    if let Err(error) = fs::write(&scenario_path, scenario_toml) {
        panic!("failed writing scenario file: {error}");
    }

    let binary = env!("CARGO_BIN_EXE_aura-harness");
    let status = match Command::new(binary)
        .arg("run")
        .arg("--config")
        .arg(config_path.as_os_str())
        .arg("--scenario")
        .arg(scenario_path.as_os_str())
        .arg("--artifacts-dir")
        .arg(artifact_root.as_os_str())
        .status()
    {
        Ok(status) => status,
        Err(error) => panic!("failed running harness binary: {error}"),
    };

    assert!(status.success());

    let run_dir = artifact_root.join("harness").join("phase1-artifacts");
    assert!(run_dir.join("startup_summary.json").exists());
    assert!(run_dir.join("events.json").exists());
    assert!(run_dir.join("initial_screens.json").exists());
}

fn instance(id: &str, data_dir: std::path::PathBuf, bind_address: &str) -> InstanceConfig {
    InstanceConfig {
        id: id.to_string(),
        mode: InstanceMode::Local,
        data_dir,
        device_id: None,
        bind_address: bind_address.to_string(),
        demo_mode: false,
        command: Some("bash".to_string()),
        args: vec!["-lc".to_string(), "cat".to_string()],
        env: vec![],
        log_path: None,
        ssh_host: None,
        ssh_user: None,
        ssh_port: None,
        ssh_strict_host_key_checking: true,
        ssh_known_hosts_file: None,
        ssh_fingerprint: None,
        ssh_require_fingerprint: false,
        ssh_dry_run: true,
        remote_workdir: None,
        lan_discovery: None,
        tunnel: None,
    }
}

fn assert_ok(response: ToolResponse) {
    if let ToolResponse::Error { message } = response {
        panic!("unexpected tool response error: {message}");
    }
}

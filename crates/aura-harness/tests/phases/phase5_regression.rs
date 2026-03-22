//! Phase 5 regression tests.

#![allow(missing_docs)]

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use aura_harness::config::{
    InstanceConfig, InstanceMode, RunConfig, RunSection, RuntimeSubstrate, ScreenSource,
    TunnelConfig,
};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn phase5_run_rejects_shared_semantic_ssh_config_before_execution() {
    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir failed: {error}"),
    };

    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");
    let artifacts_dir = temp.path().join("artifacts");
    let scenario_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios/harness/real-runtime-mixed-startup-smoke.toml");

    let run_config = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "phase5-regression".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(4242),
            max_cpu_percent: None,
            max_memory_bytes: Some(1),
            max_open_files: None,
            require_remote_artifact_sync: true,
            runtime_substrate: RuntimeSubstrate::default(),
        },
        instances: vec![
            InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: temp.path().join("alice-data"),
                device_id: None,
                bind_address: "127.0.0.1:52001".to_string(),
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
            },
            InstanceConfig {
                id: "bob".to_string(),
                mode: InstanceMode::Ssh,
                data_dir: temp.path().join("bob-data"),
                device_id: None,
                bind_address: "0.0.0.0:52002".to_string(),
                demo_mode: false,
                command: None,
                args: vec![],
                env: vec![],
                log_path: None,
                ssh_host: Some("example.org".to_string()),
                ssh_user: Some("dev".to_string()),
                ssh_port: Some(22),
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: Some("SHA256:test".to_string()),
                ssh_require_fingerprint: true,
                ssh_dry_run: true,
                remote_workdir: Some("/home/dev/aura".into()),
                lan_discovery: None,
                tunnel: Some(TunnelConfig {
                    kind: "ssh".to_string(),
                    local_forward: vec!["62102:127.0.0.1:52002".to_string()],
                }),
            },
        ],
    };
    let config_body = toml::to_string(&run_config)
        .unwrap_or_else(|error| panic!("serialize run config failed: {error}"));
    if let Err(error) = fs::write(&config_path, config_body) {
        panic!("failed writing run config: {error}");
    }
    if let Err(error) = fs::write(
        &scenario_path,
        r#"id = "phase5-shared-semantic-ssh"
goal = "shared semantic ssh configurations fail in preflight"

[[steps]]
id = "launch"
action = "launch_actors"
"#,
    ) {
        panic!("failed writing scenario file: {error}");
    }

    let binary = env!("CARGO_BIN_EXE_aura-harness");
    let output = match Command::new(binary)
        .arg("run")
        .arg("--config")
        .arg(config_path.as_os_str())
        .arg("--scenario")
        .arg(scenario_path.as_os_str())
        .arg("--artifacts-dir")
        .arg(artifacts_dir.as_os_str())
        .output()
    {
        Ok(output) => output,
        Err(error) => panic!("failed running harness binary: {error}"),
    };
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("shared semantic scenarios require explicit shared-semantic backends"));
}

#[allow(clippy::disallowed_methods)]
#[test]
fn wait_for_timeout_uses_wall_clock_budget_under_continuous_output() {
    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir failed: {error}"),
    };
    let config_path = temp.path().join("run.toml");
    let config_body = format!(
        r#"schema_version = 1

[run]
name = "phase5-wait-timeout-budget"
pty_rows = 40
pty_cols = 120

[[instances]]
id = "alice"
mode = "local"
data_dir = "{}"
bind_address = "127.0.0.1:52003"
command = "bash"
args = ["-lc", "yes churn"]
"#,
        temp.path().join("alice-data").display()
    );
    if let Err(error) = fs::write(&config_path, config_body) {
        panic!("failed writing run config: {error}");
    }

    let run_config = match aura_harness::load_and_validate_run_config(&config_path) {
        Ok(config) => config,
        Err(error) => panic!("failed to load run config: {error}"),
    };
    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("failed to build coordinator: {error}"),
    };
    let mut tool_api = ToolApi::new(coordinator);
    if let Err(error) = tool_api.start_all() {
        panic!("failed to start tool api: {error}");
    }

    let started_at = Instant::now();
    let response = tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "__never_matches__".to_string(),
        selector: None,
        timeout_ms: 500,
        screen_source: ScreenSource::Default,
    });
    let elapsed = started_at.elapsed();

    if let Err(error) = tool_api.stop_all() {
        panic!("failed to stop tool api: {error}");
    }

    match response {
        ToolResponse::Error { message } => {
            assert!(message.contains("wait_for timed out"));
        }
        ToolResponse::Ok { payload } => {
            panic!("expected wait_for timeout, got success payload: {payload:?}");
        }
    }

    assert!(
        elapsed < Duration::from_millis(3500),
        "wait_for exceeded wall-clock budget: elapsed_ms={}",
        elapsed.as_millis()
    );
}

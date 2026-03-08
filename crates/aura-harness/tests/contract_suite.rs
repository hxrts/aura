#![allow(missing_docs)]

use std::path::PathBuf;
use std::process::Command;

use aura_harness::config::{
    InstanceConfig, InstanceMode, RunConfig, RunSection, ScreenSource, TunnelConfig,
};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::determinism::build_seed_bundle;
use aura_harness::replay::{ReplayBundle, ReplayRunner, REPLAY_SCHEMA_VERSION};
use aura_harness::routing::AddressResolver;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn contract_pty_control_path() {
    let run_config = local_run_config("contract-pty", 51001);
    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }

    assert_ok(api.handle_request(ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "contract-pty\n".to_string(),
    }));
    assert_ok(api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "contract-pty".to_string(),
        selector: None,
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }));

    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }
}

#[test]
fn contract_ssh_dry_run_lifecycle() {
    let run_config = mixed_run_config("contract-ssh", 51002, 51003);
    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }
    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }
}

#[test]
fn contract_replay_and_artifacts_subsystems() {
    let run_config = local_run_config("contract-replay", 51004);
    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }

    assert_ok(api.handle_request(ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "contract-replay\n".to_string(),
    }));
    assert_ok(api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "contract-replay".to_string(),
        selector: None,
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }));

    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let replay_bundle = ReplayBundle {
        schema_version: REPLAY_SCHEMA_VERSION,
        tool_api_version: "1.0".to_string(),
        run_config: run_config.clone(),
        actions: api.action_log(),
        routing_metadata: run_config
            .instances
            .iter()
            .map(|instance| AddressResolver::resolve(instance, &instance.bind_address))
            .collect(),
        seed_bundle: build_seed_bundle(&run_config),
    };

    let outcome = match ReplayRunner::execute(&replay_bundle) {
        Ok(outcome) => outcome,
        Err(error) => panic!("replay failed: {error}"),
    };
    assert_eq!(outcome.mismatches, 0);

    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir failed: {error}"),
    };
    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");
    let artifacts_dir = temp.path().join("artifacts");

    let config_body = format!(
        r#"schema_version = 1

[run]
name = "contract-artifacts"
pty_rows = 40
pty_cols = 120
seed = 101
max_cpu_percent = 100

[[instances]]
id = "alice"
mode = "local"
data_dir = "{}"
bind_address = "127.0.0.1:51005"
command = "bash"
args = ["-lc", "cat"]
"#,
        temp.path().join("alice-data").display()
    );
    if let Err(error) = std::fs::write(&config_path, config_body) {
        panic!("write config failed: {error}");
    }

    let scenario_body = r#"schema_version = 1
id = "contract-artifacts"
goal = "emit artifacts"
execution_mode = "scripted"

[[steps]]
id = "noop"
action = "noop"
"#;
    if let Err(error) = std::fs::write(&scenario_path, scenario_body) {
        panic!("write scenario failed: {error}");
    }

    let binary = env!("CARGO_BIN_EXE_aura-harness");
    let status = match Command::new(binary)
        .arg("run")
        .arg("--config")
        .arg(config_path.as_os_str())
        .arg("--scenario")
        .arg(scenario_path.as_os_str())
        .arg("--artifacts-dir")
        .arg(artifacts_dir.as_os_str())
        .status()
    {
        Ok(status) => status,
        Err(error) => panic!("run command failed: {error}"),
    };
    assert!(status.success());

    let run_dir = artifacts_dir.join("harness").join("contract-artifacts");
    assert!(run_dir.join("startup_summary.json").exists());
    assert!(run_dir.join("replay_bundle.json").exists());
    assert!(run_dir.join("seed_bundle.json").exists());
    assert!(run_dir.join("resource_report.json").exists());
    assert!(run_dir.join("remote_artifact_sync.json").exists());
}

fn local_run_config(name: &str, port: u16) -> RunConfig {
    RunConfig {
        schema_version: 1,
        run: RunSection {
            name: name.to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(7),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
                runtime_substrate: Default::default(),
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: PathBuf::from(format!("/tmp/{name}-alice")),
            device_id: None,
            bind_address: format!("127.0.0.1:{port}"),
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
        }],
    }
}

fn mixed_run_config(name: &str, local_port: u16, ssh_port: u16) -> RunConfig {
    RunConfig {
        schema_version: 1,
        run: RunSection {
            name: name.to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(9),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
                runtime_substrate: Default::default(),
        },
        instances: vec![
            InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: PathBuf::from(format!("/tmp/{name}-alice")),
                device_id: None,
                bind_address: format!("127.0.0.1:{local_port}"),
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
                data_dir: PathBuf::from("/tmp/contract-ssh-bob"),
                device_id: None,
                bind_address: format!("0.0.0.0:{ssh_port}"),
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
                remote_workdir: Some(PathBuf::from("/home/dev/aura")),
                lan_discovery: None,
                tunnel: Some(TunnelConfig {
                    kind: "ssh".to_string(),
                    local_forward: vec![format!("{}:127.0.0.1:{}", ssh_port + 100, ssh_port)],
                }),
            },
        ],
    }
}

fn assert_ok(response: ToolResponse) {
    if let ToolResponse::Error { message } = response {
        panic!("unexpected tool response error: {message}");
    }
}

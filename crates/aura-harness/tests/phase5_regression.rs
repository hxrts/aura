#![allow(missing_docs)]

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use aura_harness::api_version::TOOL_API_VERSIONS;
use aura_harness::artifact_sync::RemoteArtifactSyncReport;
use aura_harness::config::{
    InstanceConfig, InstanceMode, RunConfig, RunSection, ScreenSource, TunnelConfig,
};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::determinism::{build_seed_bundle, SeedBundle};
use aura_harness::replay::ReplayBundle;
use aura_harness::resource_guards::ResourceGuardReport;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn phase5_run_emits_hardening_artifacts_with_seed_and_sync_metadata() {
    let temp = match tempfile::tempdir() {
        Ok(temp) => temp,
        Err(error) => panic!("tempdir failed: {error}"),
    };

    let config_path = temp.path().join("run.toml");
    let scenario_path = temp.path().join("scenario.toml");
    let artifacts_dir = temp.path().join("artifacts");

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
            runtime_substrate: Default::default(),
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

    let scenario_body = r#"id = "phase5-regression"
goal = "validate phase 5 hardening artifacts"

[[steps]]
id = "seed"
action = "set_var"
name = "artifact_seed"
value = "ok"
"#;
    if let Err(error) = fs::write(&scenario_path, scenario_body) {
        panic!("failed writing scenario config: {error}");
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
        Err(error) => panic!("failed running harness binary: {error}"),
    };
    assert!(status.success());

    let run_dir = artifacts_dir.join("harness").join("phase5-regression");

    let seed_bundle: SeedBundle = read_json(&run_dir.join("seed_bundle.json"));
    let replay_bundle: ReplayBundle = read_json(&run_dir.join("replay_bundle.json"));
    let resource_report: ResourceGuardReport = read_json(&run_dir.join("resource_report.json"));
    let remote_sync_report: RemoteArtifactSyncReport =
        read_json(&run_dir.join("remote_artifact_sync.json"));

    let run_config = match aura_harness::load_and_validate_run_config(&config_path) {
        Ok(config) => config,
        Err(error) => panic!("failed to reload run config: {error}"),
    };
    let expected_seed_bundle = build_seed_bundle(&run_config);

    assert_eq!(seed_bundle.run_seed, expected_seed_bundle.run_seed);
    assert_eq!(
        seed_bundle.scenario_seed,
        expected_seed_bundle.scenario_seed
    );
    assert_eq!(seed_bundle.fault_seed, expected_seed_bundle.fault_seed);
    assert_eq!(
        seed_bundle.instance_seeds,
        expected_seed_bundle.instance_seeds
    );

    assert_eq!(replay_bundle.seed_bundle.run_seed, seed_bundle.run_seed);
    assert!(TOOL_API_VERSIONS
        .iter()
        .any(|supported| *supported == replay_bundle.tool_api_version));

    assert!(resource_report
        .samples
        .iter()
        .any(|sample| sample.stage == "run_start"));
    assert!(resource_report
        .samples
        .iter()
        .any(|sample| sample.stage == "run_stop"));
    assert!(resource_report.samples.iter().any(|sample| {
        sample
            .violations
            .iter()
            .any(|violation| violation.contains("memory usage"))
    }));

    assert!(remote_sync_report.required);
    assert!(remote_sync_report.complete);
    assert_eq!(remote_sync_report.records.len(), 1);
    assert_eq!(remote_sync_report.records[0].instance_id, "bob");
    assert_eq!(remote_sync_report.records[0].status, "simulated");
    assert!(!remote_sync_report.records[0].checksum_sha256.is_empty());
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
            panic!("expected wait_for timeout, got success payload: {payload}");
        }
    }

    assert!(
        elapsed < Duration::from_millis(3500),
        "wait_for exceeded wall-clock budget: elapsed_ms={}",
        elapsed.as_millis()
    );
}

fn read_json<T>(path: &std::path::Path) -> T
where
    T: serde::de::DeserializeOwned,
{
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) => panic!("failed to read {}: {error}", path.display()),
    };
    match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => panic!("failed to parse {}: {error}", path.display()),
    }
}

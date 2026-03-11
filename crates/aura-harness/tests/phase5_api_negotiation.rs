#![allow(missing_docs)]

use std::path::PathBuf;

use aura_harness::api_version::TOOL_API_VERSIONS;
use aura_harness::config::{InstanceConfig, InstanceMode, RunConfig, RunSection, RuntimeSubstrate};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn negotiation_accepts_legacy_and_current_client_versions() {
    let run = one_local_run("api-negotiation");
    let coordinator = match HarnessCoordinator::from_run_config(&run) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);

    let response = api.handle_request(ToolRequest::Negotiate {
        client_versions: vec!["0.1".to_string(), "1.0".to_string()],
    });

    let payload = match response {
        ToolResponse::Ok { payload } => payload,
        ToolResponse::Error { message } => panic!("negotiation failed unexpectedly: {message}"),
    };

    let negotiated = payload
        .get("negotiated_version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    assert_eq!(negotiated, "1.0");
    assert_eq!(api.negotiated_version(), "1.0");
}

#[test]
fn negotiation_rejects_unknown_client_versions_with_guidance() {
    let run = one_local_run("api-negotiation-unsupported");
    let coordinator = match HarnessCoordinator::from_run_config(&run) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);

    let response = api.handle_request(ToolRequest::Negotiate {
        client_versions: vec!["9.9".to_string()],
    });

    let message = match response {
        ToolResponse::Ok { payload } => panic!("expected error, got payload: {payload}"),
        ToolResponse::Error { message } => message,
    };

    assert!(message.contains("no compatible tool api version"));
    assert!(message.contains("supported_versions"));
}

#[test]
fn supported_versions_are_exposed_for_client_compatibility_matrix() {
    let supported = ToolApi::supported_versions();
    assert!(!supported.is_empty());
    assert_eq!(supported, TOOL_API_VERSIONS);
}

fn one_local_run(name: &str) -> RunConfig {
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
            runtime_substrate: RuntimeSubstrate::default(),
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: PathBuf::from("/tmp/aura-harness-phase5-api"),
            device_id: None,
            bind_address: "127.0.0.1:48001".to_string(),
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

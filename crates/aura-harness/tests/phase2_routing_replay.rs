#![allow(missing_docs)]

use std::path::PathBuf;

use aura_harness::config::{
    InstanceConfig, InstanceMode, RunConfig, RunSection, ScreenSource, TunnelConfig,
};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::determinism::build_seed_bundle;
use aura_harness::replay::{parse_bundle, ReplayBundle, ReplayRunner, REPLAY_SCHEMA_VERSION};
use aura_harness::routing::AddressResolver;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};

#[test]
fn address_resolution_rewrites_ssh_tunnel_mappings() {
    let instance = ssh_instance();
    let resolved = AddressResolver::resolve(&instance, "127.0.0.1:41001");
    assert_eq!(resolved.route, "ssh_tunnel_rewrite");
    assert_eq!(resolved.resolved_address, "127.0.0.1:54101");
}

#[test]
fn replay_runner_reexecutes_recorded_actions_without_llm() {
    let temp_root = std::env::temp_dir().join("aura-harness-phase2-replay");
    let _ = std::fs::create_dir_all(&temp_root);

    let run_config = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "phase2-replay".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(8),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: aura_harness::config::RuntimeSubstrate::default(),
        },
        instances: vec![local_instance(
            "alice",
            temp_root.join("alice"),
            "127.0.0.1:44001",
        )],
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
        keys: "phase2-replay\n".to_string(),
    }));
    assert_ok(tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "phase2-replay".to_string(),
        selector: None,
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }));
    if let Err(error) = tool_api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let bundle = ReplayBundle {
        schema_version: REPLAY_SCHEMA_VERSION,
        tool_api_version: "1.0".to_string(),
        run_config: run_config.clone(),
        actions: tool_api.action_log(),
        routing_metadata: run_config
            .instances
            .iter()
            .map(|instance| AddressResolver::resolve(instance, &instance.bind_address))
            .collect(),
        seed_bundle: build_seed_bundle(&run_config),
    };

    let encoded = match serde_json::to_string_pretty(&bundle) {
        Ok(encoded) => encoded,
        Err(error) => panic!("bundle encode failed: {error}"),
    };
    let parsed = match parse_bundle(&encoded) {
        Ok(parsed) => parsed,
        Err(error) => panic!("bundle parse failed: {error}"),
    };
    let outcome = match ReplayRunner::execute(&parsed) {
        Ok(outcome) => outcome,
        Err(error) => panic!("replay execute failed: {error}"),
    };

    assert_eq!(outcome.actions_executed, bundle.actions.len() as u64);
    assert_eq!(outcome.mismatches, 0);
}

fn local_instance(id: &str, data_dir: PathBuf, bind_address: &str) -> InstanceConfig {
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

fn ssh_instance() -> InstanceConfig {
    InstanceConfig {
        id: "bob".to_string(),
        mode: InstanceMode::Ssh,
        data_dir: PathBuf::from("/tmp/bob"),
        device_id: None,
        bind_address: "0.0.0.0:41001".to_string(),
        demo_mode: false,
        command: None,
        args: vec![],
        env: vec![],
        log_path: None,
        ssh_host: Some("devbox-b".to_string()),
        ssh_user: Some("dev".to_string()),
        ssh_port: Some(22),
        ssh_strict_host_key_checking: true,
        ssh_known_hosts_file: Some(PathBuf::from("/tmp/known_hosts")),
        ssh_fingerprint: Some("SHA256:test".to_string()),
        ssh_require_fingerprint: true,
        ssh_dry_run: true,
        remote_workdir: Some(PathBuf::from("/home/dev/aura")),
        lan_discovery: None,
        tunnel: Some(TunnelConfig {
            kind: "ssh".to_string(),
            local_forward: vec!["54101:127.0.0.1:41001".to_string()],
        }),
    }
}

fn assert_ok(response: ToolResponse) {
    if let ToolResponse::Error { message } = response {
        panic!("unexpected tool response error: {message}");
    }
}

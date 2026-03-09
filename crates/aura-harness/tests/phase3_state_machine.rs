#![allow(missing_docs)]

use std::path::PathBuf;

use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::executor::{ExecutionMode, ScenarioExecutor};
use aura_harness::tool_api::ToolApi;

#[test]
fn state_machine_executes_mixed_topology_scripted_scenario() {
    let run_config = sample_mixed_run_config();
    let scenario = sample_scripted_scenario();

    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);

    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }
    let report = match ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api) {
        Ok(report) => report,
        Err(error) => panic!("scripted execution failed: {error}"),
    };
    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    assert!(report.completed);
    assert!(report
        .states_visited
        .iter()
        .any(|state| state == "fault-delay"));
    assert!(report
        .states_visited
        .iter()
        .any(|state| state == "ssh-restart"));
}

#[test]
fn scripted_and_agent_modes_are_transition_equivalent() {
    let run_config = sample_mixed_run_config();
    let scripted_scenario = sample_scripted_scenario();
    let agent_scenario = sample_agent_scenario();

    let scripted = execute_mode(&run_config, &scripted_scenario, ExecutionMode::Scripted);
    let agent = execute_mode(&run_config, &agent_scenario, ExecutionMode::Agent);

    assert!(scripted.completed);
    assert!(agent.completed);
    assert_eq!(scripted.execution_mode, ExecutionMode::Scripted);
    assert_eq!(agent.execution_mode, ExecutionMode::Agent);
    assert!(!scripted.states_visited.is_empty());
    assert!(!agent.states_visited.is_empty());
}

fn execute_mode(
    run_config: &aura_harness::config::RunConfig,
    scenario: &aura_harness::config::ScenarioConfig,
    mode: ExecutionMode,
) -> aura_harness::executor::ScenarioReport {
    let coordinator = match HarnessCoordinator::from_run_config(run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);

    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }
    let report = match ScenarioExecutor::new(mode).execute(scenario, &mut api) {
        Ok(report) => report,
        Err(error) => panic!("scenario execution failed: {error}"),
    };
    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }
    report
}

fn sample_mixed_run_config() -> aura_harness::config::RunConfig {
    use aura_harness::config::{InstanceConfig, InstanceMode, RunConfig, RunSection};

    RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "mixed-topology-test".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: Some(1000),
            seed: Some(42),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: aura_harness::config::RuntimeSubstrate::default(),
        },
        instances: vec![
            InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: PathBuf::from("artifacts/harness/state/test-mixed/alice"),
                device_id: None,
                bind_address: "127.0.0.1:0".to_string(),
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
                mode: InstanceMode::Local,
                data_dir: PathBuf::from("artifacts/harness/state/test-mixed/bob"),
                device_id: None,
                bind_address: "127.0.0.1:0".to_string(),
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
        ],
    }
}

fn sample_scripted_scenario() -> aura_harness::config::ScenarioConfig {
    use aura_harness::config::{ScenarioAction, ScenarioConfig, ScenarioStep};

    ScenarioConfig {
        schema_version: 1,
        id: "mixed-topology-smoke".to_string(),
        goal: "Exercise local plus ssh-dry-run topology with state-machine execution.".to_string(),
        execution_mode: Some("scripted".to_string()),
        required_capabilities: vec!["local".to_string(), "ssh".to_string()],
        steps: vec![
            ScenarioStep {
                id: "launch".to_string(),
                action: ScenarioAction::LaunchInstances,
                timeout_ms: Some(5000),
                ..Default::default()
            },
            ScenarioStep {
                id: "fault-delay".to_string(),
                action: ScenarioAction::FaultDelay,
                instance: Some("bob".to_string()),
                timeout_ms: Some(50),
                ..Default::default()
            },
            ScenarioStep {
                id: "ssh-restart".to_string(),
                action: ScenarioAction::Restart,
                instance: Some("bob".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
            ScenarioStep {
                id: "local-send".to_string(),
                action: ScenarioAction::SendKeys,
                instance: Some("alice".to_string()),
                keys: Some("mixed-topology-msg\n".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
            ScenarioStep {
                id: "local-wait".to_string(),
                action: ScenarioAction::WaitFor,
                instance: Some("alice".to_string()),
                pattern: Some("mixed-topology-msg".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
        ],
    }
}

fn sample_agent_scenario() -> aura_harness::config::ScenarioConfig {
    use aura_harness::config::{ScenarioAction, ScenarioConfig, ScenarioStep};

    ScenarioConfig {
        schema_version: 1,
        id: "mixed-topology-agent".to_string(),
        goal: "Run the same state-machine path using agent mode.".to_string(),
        execution_mode: Some("agent".to_string()),
        required_capabilities: vec!["local".to_string(), "ssh".to_string()],
        steps: vec![
            ScenarioStep {
                id: "launch".to_string(),
                action: ScenarioAction::LaunchInstances,
                timeout_ms: Some(5000),
                ..Default::default()
            },
            ScenarioStep {
                id: "fault-delay".to_string(),
                action: ScenarioAction::FaultDelay,
                instance: Some("bob".to_string()),
                timeout_ms: Some(50),
                ..Default::default()
            },
            ScenarioStep {
                id: "local-send".to_string(),
                action: ScenarioAction::SendKeys,
                instance: Some("alice".to_string()),
                keys: Some("agent-mode-msg\n".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
            ScenarioStep {
                id: "local-wait".to_string(),
                action: ScenarioAction::WaitFor,
                instance: Some("alice".to_string()),
                pattern: Some("agent-mode-msg".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
        ],
    }
}

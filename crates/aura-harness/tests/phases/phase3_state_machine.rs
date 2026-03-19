//! Phase 3 state machine transition tests.

#![allow(missing_docs)]

use std::path::PathBuf;
use std::sync::OnceLock;

use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::executor::{ExecutionMode, ScenarioExecutor};
use aura_harness::tool_api::ToolApi;

#[allow(clippy::disallowed_types)]
fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_MUTEX: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    TEST_MUTEX
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[test]
fn compatibility_state_machine_executes_mixed_topology_scenario() {
    let _guard = test_guard();
    let run_config = sample_mixed_run_config();
    let scenario = sample_compatibility_scenario();

    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut api = ToolApi::new(coordinator);

    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }
    let report =
        match ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api) {
            Ok(report) => report,
            Err(error) => panic!("compatibility execution failed: {error}"),
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
        .any(|state| state == "local-send"));
    assert_eq!(
        report.step_metrics.len(),
        scenario.compatibility_steps.len()
    );
    assert!(report.total_duration_ms > 0);
}

#[test]
fn compatibility_and_agent_modes_are_transition_equivalent() {
    let _guard = test_guard();
    let run_config = sample_mixed_run_config();
    let compatibility_scenario = sample_compatibility_scenario();
    let agent_scenario = sample_agent_scenario();

    let compatibility = execute_mode(
        &run_config,
        &compatibility_scenario,
        ExecutionMode::Compatibility,
    );
    let agent = execute_mode(&run_config, &agent_scenario, ExecutionMode::Agent);

    assert!(compatibility.completed);
    assert!(agent.completed);
    assert_eq!(compatibility.execution_mode, ExecutionMode::Compatibility);
    assert_eq!(agent.execution_mode, ExecutionMode::Agent);
    assert!(!compatibility.states_visited.is_empty());
    assert!(!agent.states_visited.is_empty());
    assert_eq!(
        compatibility.step_metrics.len(),
        compatibility_scenario.compatibility_steps.len()
    );
    assert_eq!(
        agent.step_metrics.len(),
        agent_scenario.compatibility_steps.len()
    );
    assert!(compatibility.total_duration_ms > 0);
    assert!(agent.total_duration_ms > 0);
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
    use aura_harness::config::{
        InstanceConfig, InstanceMode, RunConfig, RunSection, RuntimeSubstrate,
    };

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
            runtime_substrate: RuntimeSubstrate::default(),
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

fn sample_compatibility_scenario() -> aura_harness::config::ScenarioConfig {
    use aura_harness::config::{CompatibilityAction, CompatibilityStep, ScenarioConfig};

    ScenarioConfig {
        schema_version: 1,
        id: "mixed-topology-smoke".to_string(),
        goal: "Exercise local plus ssh-dry-run topology with compatibility execution.".to_string(),
        execution_mode: Some("compatibility".to_string()),
        required_capabilities: vec!["local".to_string(), "ssh".to_string()],
        compatibility_steps: vec![
            CompatibilityStep {
                id: "launch".to_string(),
                action: CompatibilityAction::LaunchInstances,
                timeout_ms: Some(5000),
                ..Default::default()
            },
            CompatibilityStep {
                id: "fault-delay".to_string(),
                action: CompatibilityAction::FaultDelay,
                instance: Some("bob".to_string()),
                timeout_ms: Some(50),
                ..Default::default()
            },
            CompatibilityStep {
                id: "local-send".to_string(),
                action: CompatibilityAction::SendKeys,
                instance: Some("alice".to_string()),
                keys: Some("mixed-topology-msg\n".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
            CompatibilityStep {
                id: "local-wait".to_string(),
                action: CompatibilityAction::WaitFor,
                instance: Some("alice".to_string()),
                pattern: Some("mixed-topology-msg".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
        ],
        semantic_steps: Vec::new(),
    }
}

fn sample_agent_scenario() -> aura_harness::config::ScenarioConfig {
    use aura_harness::config::{CompatibilityAction, CompatibilityStep, ScenarioConfig};

    ScenarioConfig {
        schema_version: 1,
        id: "mixed-topology-agent".to_string(),
        goal: "Run the same compatibility path using agent mode.".to_string(),
        execution_mode: Some("agent".to_string()),
        required_capabilities: vec!["local".to_string(), "ssh".to_string()],
        compatibility_steps: vec![
            CompatibilityStep {
                id: "launch".to_string(),
                action: CompatibilityAction::LaunchInstances,
                timeout_ms: Some(5000),
                ..Default::default()
            },
            CompatibilityStep {
                id: "fault-delay".to_string(),
                action: CompatibilityAction::FaultDelay,
                instance: Some("bob".to_string()),
                timeout_ms: Some(50),
                ..Default::default()
            },
            CompatibilityStep {
                id: "local-send".to_string(),
                action: CompatibilityAction::SendKeys,
                instance: Some("alice".to_string()),
                keys: Some("agent-mode-msg\n".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
            CompatibilityStep {
                id: "local-wait".to_string(),
                action: CompatibilityAction::WaitFor,
                instance: Some("alice".to_string()),
                pattern: Some("agent-mode-msg".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            },
        ],
        semantic_steps: Vec::new(),
    }
}

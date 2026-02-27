#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::executor::{ExecutionMode, ScenarioExecutor};
use aura_harness::scenario::ScenarioRunner;
use aura_harness::tool_api::ToolApi;

#[test]
fn state_machine_executes_mixed_topology_scripted_scenario() {
    let run_config = load_run_config(sample_mixed_run_config());
    let scenario = load_scenario(sample_scripted_scenario());

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
    let run_config = load_run_config(sample_mixed_run_config());
    let scripted_scenario = load_scenario(sample_scripted_scenario());
    let agent_scenario = load_scenario(sample_agent_scenario());

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

fn load_run_config(path: &Path) -> aura_harness::config::RunConfig {
    match aura_harness::load_and_validate_run_config(path) {
        Ok(config) => config,
        Err(error) => panic!("run config load failed: {error}"),
    }
}

fn load_scenario(path: &Path) -> aura_harness::config::ScenarioConfig {
    match ScenarioRunner::load_and_validate(path) {
        Ok(config) => config,
        Err(error) => panic!("scenario load failed: {error}"),
    }
}

fn sample_mixed_run_config() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("harness")
            .join("local-plus-ssh.toml")
    })
}

fn sample_scripted_scenario() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("scenarios")
            .join("harness")
            .join("mixed-topology-smoke.toml")
    })
}

fn sample_agent_scenario() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("scenarios")
            .join("harness")
            .join("mixed-topology-agent.toml")
    })
}

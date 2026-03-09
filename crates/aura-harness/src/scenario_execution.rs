//! High-level scenario execution with budget enforcement.
//!
//! Orchestrates scenario execution with deterministic seeds and time budgets,
//! integrating linting, seed derivation, and executor invocation.

use anyhow::{anyhow, bail, Result};

use crate::config::{RunConfig, ScenarioConfig};
use crate::determinism::build_seed_bundle;
use crate::executor::{ExecutionBudgets, ScenarioExecutor, ScenarioReport};
use crate::scenario::ScenarioRunner;
use crate::tool_api::ToolApi;

/// Validate scenario references/actions against the active run config.
pub fn lint_for_run(run_config: &RunConfig, scenario: &ScenarioConfig) -> Result<()> {
    let lint = ScenarioRunner::lint(run_config, scenario);
    if !lint.errors.is_empty() {
        bail!("scenario lint failed: {}", lint.errors.join(" | "));
    }
    Ok(())
}

/// Execute a scenario with budgets derived from the run config and deterministic seeds.
pub fn execute_with_run_budgets(
    run_config: &RunConfig,
    scenario: &ScenarioConfig,
    tool_api: &mut ToolApi,
) -> Result<ScenarioReport> {
    lint_for_run(run_config, scenario)?;
    let seed_bundle = build_seed_bundle(run_config);
    let executor = ScenarioExecutor::from_config(scenario);
    let budgets = ExecutionBudgets {
        global_budget_ms: run_config.run.global_budget_ms,
        default_step_budget_ms: run_config.run.step_budget_ms.unwrap_or(2000),
        scenario_seed: seed_bundle.scenario_seed,
        fault_seed: seed_bundle.fault_seed,
    };
    executor
        .execute_with_budgets(scenario, tool_api, budgets)
        .map_err(|error| anyhow!("scenario execution failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        InstanceConfig, InstanceMode, RunSection, RuntimeSubstrate, ScenarioAction, ScenarioStep,
    };
    use crate::coordinator::HarnessCoordinator;
    use std::path::PathBuf;

    fn test_run_config(data_dir: PathBuf) -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "scenario-execution-test".to_string(),
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
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir,
                device_id: None,
                bind_address: "127.0.0.1:45101".to_string(),
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

    #[test]
    fn lint_for_run_rejects_unknown_instance() {
        let temp_dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let run_config = test_run_config(temp_dir.path().join("alice"));
        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "lint-failure".to_string(),
            goal: "unknown instance".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendKeys,
                instance: Some("bob".to_string()),
                expect: Some("hello".to_string()),
                timeout_ms: Some(250),
                ..Default::default()
            }],
        };

        let error = match lint_for_run(&run_config, &scenario) {
            Ok(()) => panic!("unknown instance should fail lint"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("unknown instance"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn execute_with_run_budgets_runs_noop_scenario() {
        let temp_dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let run_config = test_run_config(temp_dir.path().join("alice"));
        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "noop".to_string(),
            goal: "noop".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::Noop,
                instance: None,
                expect: None,
                timeout_ms: Some(100),
                ..Default::default()
            }],
        };

        let coordinator = HarnessCoordinator::from_run_config(&run_config)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut tool_api = ToolApi::new(coordinator);
        if let Err(error) = tool_api.start_all() {
            panic!("start_all failed: {error}");
        }

        let report = execute_with_run_budgets(&run_config, &scenario, &mut tool_api)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(report.completed);
        assert_eq!(report.states_visited, vec!["step-1".to_string()]);

        if let Err(error) = tool_api.stop_all() {
            panic!("stop_all failed: {error}");
        }
    }

    #[test]
    fn execute_with_run_budgets_supports_simulator_substrate_faults() {
        let temp_dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let mut run_config = test_run_config(temp_dir.path().join("alice"));
        run_config.run.runtime_substrate = RuntimeSubstrate::Simulator;
        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "sim-fault".to_string(),
            goal: "simulator substrate fault".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::FaultDelay,
                instance: Some("alice".to_string()),
                expect: None,
                timeout_ms: Some(10),
                ..Default::default()
            }],
        };

        let coordinator = HarnessCoordinator::from_run_config(&run_config)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut tool_api = ToolApi::new(coordinator);

        let report = execute_with_run_budgets(&run_config, &scenario, &mut tool_api)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(report.completed);
        assert_eq!(tool_api.runtime_substrate(), RuntimeSubstrate::Simulator);
    }
}

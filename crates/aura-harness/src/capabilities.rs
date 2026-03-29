//! Capability detection and requirement checking for test scenarios.
//!
//! Determines available harness backend and lane capabilities from the run config
//! and rejects unsupported semantic-lane/backend combinations before execution.

use std::collections::BTreeSet;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::config::{InstanceMode, RunConfig, ScenarioConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCapability {
    Local,
    Browser,
    Ssh,
    Simulator,
    SharedSemanticLane,
    RawUiLane,
    DiagnosticObservation,
}

impl HarnessCapability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Browser => "browser",
            Self::Ssh => "ssh",
            Self::Simulator => "simulator",
            Self::SharedSemanticLane => "shared_semantic_lane",
            Self::RawUiLane => "raw_ui_lane",
            Self::DiagnosticObservation => "diagnostic_observation",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "browser" => Some(Self::Browser),
            "ssh" => Some(Self::Ssh),
            "simulator" => Some(Self::Simulator),
            "shared_semantic_lane" => Some(Self::SharedSemanticLane),
            "raw_ui_lane" => Some(Self::RawUiLane),
            "diagnostic_observation" => Some(Self::DiagnosticObservation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    pub available: Vec<String>,
    pub required: Vec<String>,
}

pub fn available_capabilities(run_config: &RunConfig) -> BTreeSet<HarnessCapability> {
    let mut capabilities = BTreeSet::new();

    let has_local = run_config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Local));
    let has_browser = run_config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Browser));
    let has_ssh = run_config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Ssh));

    if has_local || has_browser {
        capabilities.insert(HarnessCapability::Local);
        capabilities.insert(HarnessCapability::SharedSemanticLane);
        capabilities.insert(HarnessCapability::RawUiLane);
        capabilities.insert(HarnessCapability::DiagnosticObservation);
    }
    if has_browser {
        capabilities.insert(HarnessCapability::Browser);
    }
    if has_ssh {
        capabilities.insert(HarnessCapability::Ssh);
    }
    if run_config.run.runtime_substrate == crate::config::RuntimeSubstrate::Simulator {
        capabilities.insert(HarnessCapability::Simulator);
    }

    capabilities
}

pub fn check_scenario_capabilities(
    run_config: &RunConfig,
    scenario: Option<&ScenarioConfig>,
) -> Result<CapabilityReport> {
    let available = available_capabilities(run_config);
    let required: BTreeSet<HarnessCapability> = scenario
        .map(|scenario| scenario.required_capabilities.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|capability| {
            HarnessCapability::parse(&capability)
                .ok_or_else(|| anyhow!("unknown harness capability {capability:?}"))
        })
        .collect::<Result<_>>()?;

    if scenario.is_some_and(is_shared_semantic_scenario) {
        let unsupported = run_config
            .instances
            .iter()
            .filter(|instance| matches!(instance.mode, InstanceMode::Ssh))
            .map(|instance| instance.id.as_str())
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            bail!(
                "shared semantic scenarios require explicit shared-semantic backends; ssh instances {unsupported:?} are diagnostic-only and must not enter the semantic lane"
            );
        }
    }

    for capability in &required {
        if !available.contains(capability) {
            let available = available
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>();
            bail!(
                "scenario requires capability {:?}, but run provides {:?}",
                capability.as_str(),
                available
            );
        }
    }

    Ok(CapabilityReport {
        available: available
            .into_iter()
            .map(HarnessCapability::as_str)
            .map(str::to_string)
            .collect(),
        required: required
            .into_iter()
            .map(HarnessCapability::as_str)
            .map(str::to_string)
            .collect(),
    })
}

fn is_shared_semantic_scenario(scenario: &ScenarioConfig) -> bool {
    scenario.is_shared_semantic()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{InstanceConfig, RunSection};

    #[test]
    fn capability_matrix_reports_local_and_ssh() {
        let run_config = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "capability-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(3),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![
                base_instance("alice", InstanceMode::Local),
                base_instance("bob", InstanceMode::Ssh),
            ],
        };

        let available = available_capabilities(&run_config);
        assert!(available.contains(&HarnessCapability::Local));
        assert!(available.contains(&HarnessCapability::Ssh));
        assert!(available.contains(&HarnessCapability::SharedSemanticLane));
    }

    #[test]
    fn capability_matrix_reports_browser_as_local_compatible() {
        let run_config = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "capability-browser-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(3),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![base_instance("alice", InstanceMode::Browser)],
        };

        let available = available_capabilities(&run_config);
        assert!(available.contains(&HarnessCapability::Browser));
        assert!(available.contains(&HarnessCapability::Local));
        assert!(available.contains(&HarnessCapability::SharedSemanticLane));
        assert!(available.contains(&HarnessCapability::RawUiLane));
        assert!(available.contains(&HarnessCapability::DiagnosticObservation));
    }

    #[test]
    fn shared_semantic_scenarios_reject_ssh_instances_before_runtime() {
        let run_config = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "semantic-ssh-rejected".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(3),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![
                base_instance("alice", InstanceMode::Local),
                base_instance("bob", InstanceMode::Ssh),
            ],
        };
        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "semantic-shared".to_string(),
            goal: "semantic ssh rejection".to_string(),
            classification: None,
            execution_mode: None,
            required_capabilities: vec![],
            compatibility_steps: Vec::new(),
            semantic_steps: vec![aura_app::scenario_contract::ScenarioStep {
                id: "step-1".to_string(),
                actor: None,
                action: aura_app::scenario_contract::ScenarioAction::Environment(
                    aura_app::scenario_contract::EnvironmentAction::LaunchActors,
                ),
                timeout_ms: Some(100),
            }],
        };

        let error = match check_scenario_capabilities(&run_config, Some(&scenario)) {
            Ok(_) => panic!("shared semantic ssh combinations must fail early"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("diagnostic-only"));
    }

    fn base_instance(id: &str, mode: InstanceMode) -> InstanceConfig {
        InstanceConfig {
            id: id.to_string(),
            mode,
            data_dir: PathBuf::from("/tmp/test"),
            device_id: None,
            bind_address: "127.0.0.1:41001".to_string(),
            demo_mode: false,
            command: None,
            args: vec![],
            env: vec![],
            log_path: None,
            ssh_host: Some("example.org".to_string()),
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: Some(PathBuf::from("/home/dev/aura")),
            lan_discovery: None,
            tunnel: None,
        }
    }
}

//! Capability detection and requirement checking for test scenarios.
//!
//! Determines available test capabilities (local, browser, SSH, remote-only) based
//! on instance configuration and validates that scenarios have required capabilities.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::config::{InstanceMode, RunConfig, ScenarioConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    pub available: Vec<String>,
    pub required: Vec<String>,
}

pub fn available_capabilities(run_config: &RunConfig) -> BTreeSet<String> {
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
        capabilities.insert("local".to_string());
    }
    if has_browser {
        capabilities.insert("browser".to_string());
    }
    if has_ssh {
        capabilities.insert("ssh".to_string());
    }
    if has_ssh && !has_local && !has_browser {
        capabilities.insert("remote-only".to_string());
    }

    capabilities
}

pub fn check_scenario_capabilities(
    run_config: &RunConfig,
    scenario: Option<&ScenarioConfig>,
) -> Result<CapabilityReport> {
    let available = available_capabilities(run_config);
    let required: BTreeSet<String> = scenario
        .map(|scenario| scenario.required_capabilities.iter().cloned().collect())
        .unwrap_or_default();

    for capability in &required {
        if !available.contains(capability) {
            bail!("scenario requires capability {capability:?}, but run provides {available:?}");
        }
    }

    Ok(CapabilityReport {
        available: available.into_iter().collect(),
        required: required.into_iter().collect(),
    })
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
            },
            instances: vec![
                base_instance("alice", InstanceMode::Local),
                base_instance("bob", InstanceMode::Ssh),
            ],
        };

        let available = available_capabilities(&run_config);
        assert!(available.contains("local"));
        assert!(available.contains("ssh"));
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
            },
            instances: vec![base_instance("alice", InstanceMode::Browser)],
        };

        let available = available_capabilities(&run_config);
        assert!(available.contains("browser"));
        assert!(available.contains("local"));
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

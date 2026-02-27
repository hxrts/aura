use std::collections::BTreeSet;
use std::fs;
use std::net::TcpListener;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::capabilities::{check_scenario_capabilities, CapabilityReport};
use crate::config::{InstanceMode, RunConfig, ScenarioConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightCheck {
    pub name: String,
    pub ok: bool,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightReport {
    pub checks: Vec<PreflightCheck>,
    pub capabilities: CapabilityReport,
}

pub fn run_preflight(
    run_config: &RunConfig,
    scenario: Option<&ScenarioConfig>,
) -> Result<PreflightReport> {
    let capabilities = check_scenario_capabilities(run_config, scenario)?;

    let mut checks = Vec::new();
    checks.push(PreflightCheck {
        name: "capability_matrix".to_string(),
        ok: true,
        details: format!(
            "required={:?} available={:?}",
            capabilities.required, capabilities.available
        ),
    });

    let local_instances: Vec<_> = run_config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Local))
        .collect();
    validate_storage_isolation(&local_instances)?;
    checks.push(PreflightCheck {
        name: "storage_isolation".to_string(),
        ok: true,
        details: "all local data_dir values are unique".to_string(),
    });

    validate_writable_dirs(&local_instances)?;
    checks.push(PreflightCheck {
        name: "writable_data_dirs".to_string(),
        ok: true,
        details: "local data_dir paths are writable".to_string(),
    });

    validate_binaries(run_config)?;
    checks.push(PreflightCheck {
        name: "binary_availability".to_string(),
        ok: true,
        details: "required binaries found in PATH".to_string(),
    });

    validate_ports(&local_instances)?;
    checks.push(PreflightCheck {
        name: "port_availability".to_string(),
        ok: true,
        details: "local bind ports can be reserved".to_string(),
    });

    validate_ssh_defaults(run_config)?;
    checks.push(PreflightCheck {
        name: "ssh_security_defaults".to_string(),
        ok: true,
        details: "strict host key checks and fingerprint policy validated".to_string(),
    });

    Ok(PreflightReport {
        checks,
        capabilities,
    })
}

fn validate_storage_isolation(local_instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    let mut dirs = BTreeSet::new();
    for instance in local_instances {
        if !dirs.insert(instance.data_dir.clone()) {
            bail!(
                "preflight rejected duplicate local data_dir {}",
                instance.data_dir.display()
            );
        }
    }

    let mut demo_dirs = BTreeSet::new();
    for instance in local_instances {
        if instance.demo_mode
            && instance.data_dir.to_string_lossy().contains(".aura-demo")
            && !demo_dirs.insert(instance.data_dir.clone())
        {
            bail!(
                "preflight rejected shared demo-mode data_dir {}",
                instance.data_dir.display()
            );
        }
    }

    Ok(())
}

fn validate_writable_dirs(local_instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    for instance in local_instances {
        fs::create_dir_all(&instance.data_dir).with_context(|| {
            format!(
                "failed to create local data_dir {}",
                instance.data_dir.display()
            )
        })?;
        let probe_path = instance.data_dir.join(".harness-preflight-probe");
        fs::write(&probe_path, b"ok").with_context(|| {
            format!(
                "failed to write preflight probe in {}",
                instance.data_dir.display()
            )
        })?;
        let _ = fs::remove_file(probe_path);
    }
    Ok(())
}

fn validate_binaries(run_config: &RunConfig) -> Result<()> {
    let needs_ssh = run_config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Ssh));
    if needs_ssh {
        require_binary("ssh")?;
    }

    for instance in run_config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Local))
    {
        if let Some(command) = &instance.command {
            require_binary(command)?;
        } else {
            require_binary("bash")?;
        }
    }

    Ok(())
}

fn validate_ports(local_instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    let mut listeners = Vec::new();
    for instance in local_instances {
        let bind_address = normalize_bind_address(&instance.bind_address)?;
        let listener = TcpListener::bind(&bind_address).with_context(|| {
            format!(
                "failed to reserve local bind address {bind_address} for instance {}",
                instance.id
            )
        })?;
        listeners.push(listener);
    }
    Ok(())
}

fn validate_ssh_defaults(run_config: &RunConfig) -> Result<()> {
    for instance in run_config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Ssh))
    {
        if !instance.ssh_strict_host_key_checking {
            bail!(
                "instance {} disables ssh_strict_host_key_checking",
                instance.id
            );
        }
        if instance.ssh_require_fingerprint
            && instance
                .ssh_fingerprint
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            bail!(
                "instance {} requires ssh_fingerprint when ssh_require_fingerprint is enabled",
                instance.id
            );
        }
    }
    Ok(())
}

fn normalize_bind_address(raw: &str) -> Result<String> {
    if raw.contains(':') {
        return Ok(raw.to_string());
    }
    Err(anyhow!("invalid bind address: {raw}"))
}

fn require_binary(binary: &str) -> Result<()> {
    if Path::new(binary).is_absolute() && Path::new(binary).exists() {
        return Ok(());
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.exists() {
            return Ok(());
        }
    }

    bail!("required binary not found in PATH: {binary}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunSection, ScenarioConfig, ScenarioStep};

    #[test]
    fn preflight_rejects_missing_required_capability() {
        let run = local_only_run();
        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "capability-fail".to_string(),
            goal: "require ssh".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec!["ssh".to_string()],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: "noop".to_string(),
                instance: None,
                expect: None,
                timeout_ms: None,
            }],
        };

        let error = match run_preflight(&run, Some(&scenario)) {
            Ok(_) => panic!("preflight must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("requires capability"));
    }

    #[test]
    fn preflight_rejects_duplicate_storage_paths() {
        let mut run = local_only_run();
        run.instances.push(run.instances[0].clone());
        run.instances[1].id = "bob".to_string();

        let error = match run_preflight(&run, None) {
            Ok(_) => panic!("preflight must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("duplicate local data_dir"));
    }

    fn local_only_run() -> RunConfig {
        let temp_root = std::env::temp_dir().join("aura-harness-preflight");
        let local = InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: temp_root.join("alice"),
            device_id: None,
            bind_address: "127.0.0.1:46001".to_string(),
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
        };

        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "preflight-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
            },
            instances: vec![local],
        }
    }
}

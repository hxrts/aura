//! Pre-flight checks for test environment validation.
//!
//! Validates system prerequisites (binaries, connectivity, permissions) before
//! scenario execution to fail fast with actionable diagnostics.

use std::collections::BTreeSet;
use std::fs;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::capabilities::{check_scenario_capabilities, CapabilityReport};
use crate::config::{InstanceMode, RunConfig, RuntimeSubstrate, ScenarioConfig};

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

    validate_runtime_substrate(run_config)?;
    checks.push(PreflightCheck {
        name: "runtime_substrate".to_string(),
        ok: true,
        details: format!("runtime_substrate={:?}", run_config.run.runtime_substrate)
            .to_ascii_lowercase(),
    });

    let local_instances: Vec<_> = run_config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Local))
        .collect();
    let browser_instances: Vec<_> = run_config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Browser))
        .collect();
    let storage_instances = local_instances
        .iter()
        .chain(browser_instances.iter())
        .copied()
        .collect::<Vec<_>>();
    validate_storage_isolation(&storage_instances)?;
    checks.push(PreflightCheck {
        name: "storage_isolation".to_string(),
        ok: true,
        details: "all local/browser data_dir values are unique".to_string(),
    });

    validate_writable_dirs(&storage_instances)?;
    checks.push(PreflightCheck {
        name: "writable_data_dirs".to_string(),
        ok: true,
        details: "local/browser data_dir paths are writable".to_string(),
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

    validate_browser_runtime(&browser_instances)?;
    checks.push(PreflightCheck {
        name: "browser_runtime".to_string(),
        ok: true,
        details: "browser mode has node/playwright/app-url prerequisites".to_string(),
    });

    Ok(PreflightReport {
        checks,
        capabilities,
    })
}

fn validate_storage_isolation(instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    let mut dirs = BTreeSet::new();
    for instance in instances {
        if !dirs.insert(instance.data_dir.clone()) {
            bail!(
                "preflight rejected duplicate local/browser data_dir {}",
                instance.data_dir.display()
            );
        }
    }

    let mut demo_dirs = BTreeSet::new();
    for instance in instances {
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

fn validate_writable_dirs(instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    for instance in instances {
        fs::create_dir_all(&instance.data_dir).with_context(|| {
            format!(
                "failed to create local/browser data_dir {}",
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
    let needs_browser = run_config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Browser));
    if needs_browser {
        require_binary("node")?;
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

fn validate_browser_runtime(browser_instances: &[&crate::config::InstanceConfig]) -> Result<()> {
    if browser_instances.is_empty() {
        return Ok(());
    }

    let default_driver = default_playwright_driver_path();
    if let Some(path) = &default_driver {
        if !path.exists() {
            bail!(
                "browser preflight missing default Playwright driver script {}",
                path.display()
            );
        }
    }

    for instance in browser_instances {
        if browser_app_url_is_explicit(instance) {
            let app_url = browser_app_url(instance);
            ensure_app_url_reachable(&app_url).with_context(|| {
                format!(
                    "browser preflight failed app_url reachability for instance {}",
                    instance.id
                )
            })?;
        }

        if instance.command.is_none() {
            let path = default_driver.as_ref().ok_or_else(|| {
                anyhow!("failed to resolve default Playwright driver path from cwd")
            })?;
            if !path.exists() {
                bail!(
                    "browser instance {} requires default driver script {}",
                    instance.id,
                    path.display()
                );
            }
        }
    }

    validate_playwright_chromium_available()?;
    Ok(())
}

fn validate_runtime_substrate(run_config: &RunConfig) -> Result<()> {
    if run_config.run.runtime_substrate == RuntimeSubstrate::Simulator
        && run_config
            .instances
            .iter()
            .any(|instance| !matches!(instance.mode, InstanceMode::Local))
    {
        bail!("simulator substrate currently supports local instances only");
    }

    Ok(())
}

fn browser_app_url(instance: &crate::config::InstanceConfig) -> String {
    env_value("AURA_HARNESS_BROWSER_APP_URL", &instance.env)
        .or_else(|| env_value("AURA_WEB_APP_URL", &instance.env))
        .or_else(|| std::env::var("AURA_HARNESS_BROWSER_APP_URL").ok())
        .or_else(|| std::env::var("AURA_WEB_APP_URL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:4173".to_string())
}

fn browser_app_url_is_explicit(instance: &crate::config::InstanceConfig) -> bool {
    env_value("AURA_HARNESS_BROWSER_APP_URL", &instance.env)
        .or_else(|| env_value("AURA_WEB_APP_URL", &instance.env))
        .or_else(|| std::env::var("AURA_HARNESS_BROWSER_APP_URL").ok())
        .or_else(|| std::env::var("AURA_WEB_APP_URL").ok())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn env_value(key: &str, env_entries: &[String]) -> Option<String> {
    env_entries.iter().find_map(|item| {
        let (entry_key, entry_value) = item.split_once('=')?;
        if entry_key.trim() == key {
            Some(entry_value.trim().to_string())
        } else {
            None
        }
    })
}

fn default_playwright_driver_path() -> Option<std::path::PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join("crates/aura-harness/playwright-driver/playwright_driver.mjs"))
}

fn ensure_app_url_reachable(app_url: &str) -> Result<()> {
    let (host, port) = parse_http_host_port(app_url)?;
    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve app_url host: {host}"))?
        .collect();
    if addrs.is_empty() {
        bail!("no socket addresses resolved for app_url host: {host}");
    }
    let timeout = Duration::from_millis(1200);
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return Ok(());
        }
    }
    bail!("failed to connect to app_url endpoint {host}:{port}");
}

fn parse_http_host_port(app_url: &str) -> Result<(String, u16)> {
    let trimmed = app_url.trim();
    let (rest, default_port) = if let Some(value) = trimmed.strip_prefix("http://") {
        (value, 80_u16)
    } else if let Some(value) = trimmed.strip_prefix("https://") {
        (value, 443_u16)
    } else {
        bail!("unsupported app_url scheme for browser preflight: {trimmed}");
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        bail!("invalid app_url authority: {trimmed}");
    }

    if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .with_context(|| format!("invalid app_url port in {trimmed}"))?;
        if host.is_empty() {
            bail!("invalid app_url host in {trimmed}");
        }
        return Ok((host.to_string(), port));
    }

    Ok((authority.to_string(), default_port))
}

fn validate_playwright_chromium_available() -> Result<()> {
    let script = "const { chromium } = require('playwright'); const p = chromium.executablePath(); if (!p) process.exit(2); process.stdout.write(p);";
    let mut command = Command::new("node");
    command.args(["-e", script]);
    if let Ok(cwd) = std::env::current_dir() {
        command.current_dir(cwd.join("crates/aura-harness/playwright-driver"));
    }
    let output = command
        .output()
        .context("failed to execute node check for Playwright chromium")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "Playwright chromium is unavailable (run npm ci and npm run install-browsers in crates/aura-harness/playwright-driver): {}",
        stderr.trim()
    )
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
    use crate::config::{
        InstanceConfig, InstanceMode, RunSection, ScenarioAction, ScenarioConfig, ScenarioStep,
    };

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
                action: ScenarioAction::Noop,
                instance: None,
                expect: None,
                timeout_ms: None,
                ..Default::default()
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
        assert!(error
            .to_string()
            .contains("duplicate local/browser data_dir"));
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
                seed: Some(4),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![local],
        }
    }
}

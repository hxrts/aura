use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const RUN_SCHEMA_VERSION: u32 = 1;
pub const SCENARIO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub schema_version: u32,
    pub run: RunSection,
    pub instances: Vec<InstanceConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSection {
    pub name: String,
    pub pty_rows: Option<u16>,
    pub pty_cols: Option<u16>,
    pub artifact_dir: Option<PathBuf>,
    pub global_budget_ms: Option<u64>,
    pub step_budget_ms: Option<u64>,
    pub seed: Option<u64>,
    pub max_cpu_percent: Option<u8>,
    pub max_memory_bytes: Option<u64>,
    pub max_open_files: Option<u64>,
    #[serde(default)]
    pub require_remote_artifact_sync: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceMode {
    Local,
    Ssh,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceConfig {
    pub id: String,
    pub mode: InstanceMode,
    pub data_dir: PathBuf,
    pub device_id: Option<String>,
    pub bind_address: String,
    #[serde(default)]
    pub demo_mode: bool,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    pub log_path: Option<PathBuf>,
    pub ssh_host: Option<String>,
    pub ssh_user: Option<String>,
    pub ssh_port: Option<u16>,
    #[serde(default = "default_true")]
    pub ssh_strict_host_key_checking: bool,
    pub ssh_known_hosts_file: Option<PathBuf>,
    pub ssh_fingerprint: Option<String>,
    #[serde(default)]
    pub ssh_require_fingerprint: bool,
    #[serde(default = "default_true")]
    pub ssh_dry_run: bool,
    pub remote_workdir: Option<PathBuf>,
    pub lan_discovery: Option<LanDiscoveryConfig>,
    pub tunnel: Option<TunnelConfig>,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanDiscoveryConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub broadcast_addr: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelConfig {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub local_forward: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    pub schema_version: u32,
    pub id: String,
    pub goal: String,
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioStep {
    pub id: String,
    pub action: String,
    pub instance: Option<String>,
    // Backward-compatible overloaded field used by scripted actions.
    // Prefer action-specific aliases in TOML (`keys`, `command`, `pattern`, `key`,
    // `source_instance`) to keep scenarios readable.
    #[serde(
        alias = "keys",
        alias = "command",
        alias = "pattern",
        alias = "key",
        alias = "source_instance"
    )]
    pub expect: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub fn load_run_config(path: &Path) -> Result<RunConfig> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("failed to read run config at {}", path.display()))?;
    let config: RunConfig = toml::from_str(&body)
        .with_context(|| format!("failed to parse run config TOML at {}", path.display()))?;
    Ok(config)
}

pub fn load_scenario_config(path: &Path) -> Result<ScenarioConfig> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("failed to read scenario config at {}", path.display()))?;
    let config: ScenarioConfig = toml::from_str(&body)
        .with_context(|| format!("failed to parse scenario config TOML at {}", path.display()))?;
    Ok(config)
}

impl RunConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != RUN_SCHEMA_VERSION {
            bail!(
                "unsupported run schema_version {}. expected {}",
                self.schema_version,
                RUN_SCHEMA_VERSION
            );
        }

        if self.run.name.trim().is_empty() {
            bail!("run.name must be non-empty");
        }

        if self.instances.is_empty() {
            bail!("at least one instance must be configured");
        }

        let mut instance_ids = HashSet::new();
        let mut local_data_dirs = HashSet::new();
        let mut local_demo_dirs = HashSet::new();

        for instance in &self.instances {
            if instance.id.trim().is_empty() {
                bail!("instance id must be non-empty");
            }
            if !instance_ids.insert(instance.id.clone()) {
                bail!("duplicate instance id: {}", instance.id);
            }
            if instance.bind_address.trim().is_empty() {
                bail!("instance {} has empty bind_address", instance.id);
            }

            match instance.mode {
                InstanceMode::Local => {
                    if !local_data_dirs.insert(instance.data_dir.clone()) {
                        bail!(
                            "duplicate local data_dir {} for instance {}",
                            instance.data_dir.display(),
                            instance.id
                        );
                    }
                    if instance.demo_mode
                        && instance.data_dir.to_string_lossy().contains(".aura-demo")
                        && !local_demo_dirs.insert(instance.data_dir.clone())
                    {
                        bail!(
                            "shared demo-mode data_dir {} is not allowed",
                            instance.data_dir.display()
                        );
                    }
                    if instance.ssh_host.is_some() || instance.remote_workdir.is_some() {
                        bail!(
                            "local instance {} must not set ssh_host or remote_workdir",
                            instance.id
                        );
                    }
                    if instance
                        .command
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(str::is_empty)
                    {
                        bail!("local instance {} has empty command", instance.id);
                    }
                }
                InstanceMode::Ssh => {
                    if instance
                        .ssh_host
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                    {
                        bail!("ssh instance {} must set ssh_host", instance.id);
                    }
                    if instance
                        .remote_workdir
                        .as_deref()
                        .map(|value| value.as_os_str().is_empty())
                        .unwrap_or(true)
                    {
                        bail!("ssh instance {} must set remote_workdir", instance.id);
                    }
                    if !instance.ssh_strict_host_key_checking {
                        bail!(
                            "ssh instance {} must keep ssh_strict_host_key_checking enabled",
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
                            "ssh instance {} requires ssh_fingerprint when ssh_require_fingerprint is true",
                            instance.id
                        );
                    }
                    if instance.command.is_some()
                        || !instance.args.is_empty()
                        || !instance.env.is_empty()
                    {
                        bail!(
                            "ssh instance {} must not set local command/args/env",
                            instance.id
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

impl ScenarioConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCENARIO_SCHEMA_VERSION {
            bail!(
                "unsupported scenario schema_version {}. expected {}",
                self.schema_version,
                SCENARIO_SCHEMA_VERSION
            );
        }

        if self.id.trim().is_empty() {
            bail!("scenario id must be non-empty");
        }
        if self.goal.trim().is_empty() {
            bail!("scenario goal must be non-empty");
        }
        if let Some(mode) = self.execution_mode.as_deref() {
            if mode != "scripted" && mode != "agent" {
                bail!("scenario execution_mode must be one of: scripted, agent");
            }
        }
        if self.steps.is_empty() {
            bail!("scenario must include at least one step");
        }

        let mut step_ids = HashSet::new();
        for step in &self.steps {
            if step.id.trim().is_empty() {
                bail!("scenario step id must be non-empty");
            }
            if !step_ids.insert(step.id.clone()) {
                bail!("duplicate scenario step id: {}", step.id);
            }
            if step.action.trim().is_empty() {
                bail!("scenario step {} has empty action", step.id);
            }
        }

        Ok(())
    }
}

pub fn require_existing_file(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("{} does not exist: {}", label, path.display()));
    }
    if !path.is_file() {
        return Err(anyhow!("{} must be a file: {}", label, path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_unknown_run_fields() {
        let body = r#"
            schema_version = 1
            unknown_key = "boom"

            [run]
            name = "demo"

            [[instances]]
            id = "alice"
            mode = "local"
            data_dir = ".tmp/alice"
            bind_address = "127.0.0.1:41001"
        "#;

        let parsed: Result<RunConfig, _> = toml::from_str(body);
        assert!(parsed.is_err());
    }

    #[test]
    fn duplicate_local_dirs_are_rejected() {
        let config = RunConfig {
            schema_version: RUN_SCHEMA_VERSION,
            run: RunSection {
                name: "test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: None,
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![
                InstanceConfig {
                    id: "alice".to_string(),
                    mode: InstanceMode::Local,
                    data_dir: PathBuf::from(".tmp/shared"),
                    device_id: None,
                    bind_address: "127.0.0.1:41001".to_string(),
                    demo_mode: false,
                    command: None,
                    args: vec![],
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
                    data_dir: PathBuf::from(".tmp/shared"),
                    device_id: None,
                    bind_address: "127.0.0.1:41002".to_string(),
                    demo_mode: false,
                    command: None,
                    args: vec![],
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
        };

        let error = match config.validate() {
            Ok(()) => panic!("duplicate paths must fail"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("duplicate local data_dir"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn scenario_requires_non_empty_steps() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "smoke".to_string(),
            goal: "test".to_string(),
            required_capabilities: vec![],
            steps: vec![],
            execution_mode: None,
        };

        let error = match config.validate() {
            Ok(()) => panic!("empty steps must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("at least one step"));
    }

    #[test]
    fn scenario_step_expect_aliases_parse_from_toml() {
        let body = r#"
            schema_version = 1
            id = "aliases"
            goal = "exercise aliases"
            execution_mode = "scripted"
            required_capabilities = []

            [[steps]]
            id = "send"
            action = "send_keys"
            instance = "alice"
            keys = "hello\n"

            [[steps]]
            id = "command"
            action = "send_chat_command"
            instance = "alice"
            command = "join slash-lab"

            [[steps]]
            id = "wait"
            action = "wait_for"
            instance = "alice"
            pattern = "slash-lab"

            [[steps]]
            id = "key"
            action = "send_key"
            instance = "alice"
            key = "esc"

            [[steps]]
            id = "clipboard"
            action = "send_clipboard"
            instance = "bob"
            source_instance = "alice"
        "#;

        let parsed: ScenarioConfig =
            toml::from_str(body).unwrap_or_else(|error| panic!("parse failed: {error}"));
        let values: Vec<Option<String>> = parsed
            .steps
            .iter()
            .map(|step| step.expect.clone())
            .collect();
        assert_eq!(
            values,
            vec![
                Some("hello\n".to_string()),
                Some("join slash-lab".to_string()),
                Some("slash-lab".to_string()),
                Some("esc".to_string()),
                Some("alice".to_string())
            ]
        );
    }
}

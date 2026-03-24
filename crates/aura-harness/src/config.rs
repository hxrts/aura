//! Configuration types for harness runs and test scenarios.
//!
//! Defines the schema for run configurations (instances, budgets, resource limits)
//! and scenario definitions (steps, assertions, timeouts) loaded from TOML files.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use aura_app::scenario_contract::{
    ActorId, Expectation, ScenarioAction as SemanticAction, ScenarioDefinition,
    ScenarioStep as SemanticStep, SemanticScenarioFile, VariableAction,
};
use serde::{Deserialize, Serialize};

pub use crate::compatibility_step::{CompatibilityAction, CompatibilityStep, ScreenSource};

pub const RUN_SCHEMA_VERSION: u32 = 1;
pub const SCENARIO_SCHEMA_VERSION: u32 = 1;
pub const HARNESS_SCENARIO_INVENTORY_PATH: &str = "scenarios/harness_inventory.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioInventoryFile {
    #[serde(rename = "version")]
    _version: u32,
    pub scenario: Vec<ScenarioInventoryEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioInventoryEntry {
    pub id: String,
    pub path: PathBuf,
    pub classification: ScenarioClassification,
    pub runtime_substrate: String,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioClassification {
    Shared,
    TuiConformance,
    WebConformance,
}

impl ScenarioClassification {
    #[must_use]
    pub const fn is_shared_semantic(self) -> bool {
        matches!(self, Self::Shared)
    }

    #[must_use]
    pub const fn is_frontend_conformance(self) -> bool {
        matches!(self, Self::TuiConformance | Self::WebConformance)
    }
}

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
    #[serde(default)]
    pub runtime_substrate: RuntimeSubstrate,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSubstrate {
    #[default]
    Real,
    Simulator,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceMode {
    Local,
    Browser,
    Ssh,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceConfig {
    pub id: String,
    pub mode: InstanceMode,
    pub data_dir: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub bind_address: String,
    #[serde(default)]
    pub demo_mode: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
    #[serde(default = "default_true")]
    pub ssh_strict_host_key_checking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_known_hosts_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_fingerprint: Option<String>,
    #[serde(default)]
    pub ssh_require_fingerprint: bool,
    #[serde(default = "default_true")]
    pub ssh_dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_workdir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_discovery: Option<LanDiscoveryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<TunnelConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioActorBinding {
    pub actor: ActorId,
    pub mode: InstanceMode,
}

const fn default_true() -> bool {
    true
}

fn is_harness_env_entry(entry: &str) -> bool {
    entry
        .split_once('=')
        .is_some_and(|(key, _)| key.starts_with("AURA_HARNESS_"))
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
    #[serde(skip)]
    pub classification: Option<ScenarioClassification>,
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(rename = "steps")]
    pub compatibility_steps: Vec<CompatibilityStep>,
    #[serde(skip)]
    pub semantic_steps: Vec<SemanticStep>,
}

impl TryFrom<ScenarioDefinition> for ScenarioConfig {
    type Error = anyhow::Error;

    fn try_from(value: ScenarioDefinition) -> Result<Self> {
        let ScenarioDefinition { id, goal, steps } = value;
        Ok(Self {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id,
            goal,
            classification: None,
            execution_mode: None,
            required_capabilities: Vec::new(),
            compatibility_steps: Vec::new(),
            semantic_steps: steps,
        })
    }
}

pub fn load_run_config(path: &Path) -> Result<RunConfig> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("failed to read run config at {}", path.display()))?;
    let config: RunConfig = toml::from_str(&body)
        .with_context(|| format!("failed to parse run config TOML at {}", path.display()))?;
    Ok(config)
}

pub fn load_scenario_inventory(path: Option<&Path>) -> Result<Vec<ScenarioInventoryEntry>> {
    let inventory_path = path.unwrap_or_else(|| Path::new(HARNESS_SCENARIO_INVENTORY_PATH));
    let body = fs::read_to_string(inventory_path).with_context(|| {
        format!(
            "failed to read scenario inventory at {}",
            inventory_path.display()
        )
    })?;
    let inventory: ScenarioInventoryFile = toml::from_str(&body).with_context(|| {
        format!(
            "failed to parse scenario inventory TOML at {}",
            inventory_path.display()
        )
    })?;
    Ok(inventory.scenario)
}

pub fn load_scenario_config(path: &Path) -> Result<ScenarioConfig> {
    let semantic = load_semantic_scenario_definition(path)
        .with_context(|| format!("failed to load semantic scenario at {}", path.display()))?;
    let classification = scenario_classification_for_path(path)?;
    let mut scenario = ScenarioConfig::try_from(semantic).with_context(|| {
        format!(
            "failed to build harness scenario model from semantic scenario {}",
            path.display()
        )
    })?;
    scenario.classification = classification;
    Ok(scenario)
}

pub fn load_semantic_scenario_definition(path: &Path) -> Result<ScenarioDefinition> {
    let body = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read semantic scenario config at {}",
            path.display()
        )
    })?;
    let file: SemanticScenarioFile = toml::from_str(&body).with_context(|| {
        format!(
            "failed to parse semantic scenario config TOML at {}",
            path.display()
        )
    })?;
    let definition = ScenarioDefinition::try_from(file).map_err(|error| {
        anyhow!(
            "failed to convert semantic scenario at {}: {error}",
            path.display()
        )
    })?;
    if scenario_classification_for_path(path)?
        .is_some_and(ScenarioClassification::is_shared_semantic)
    {
        definition
            .validate_shared_intent_contract()
            .map_err(|error| {
                anyhow!(
                    "shared scenario {} violates intent contract: {error}",
                    path.display()
                )
            })?;
    }
    Ok(definition)
}

fn scenario_classification_for_path(path: &Path) -> Result<Option<ScenarioClassification>> {
    let inventory_path = Path::new(HARNESS_SCENARIO_INVENTORY_PATH);
    if !inventory_path.exists() {
        return Ok(None);
    }
    let inventory = load_scenario_inventory(Some(inventory_path))?;
    let requested = normalize_inventory_path(path)?;
    Ok(inventory
        .into_iter()
        .find(|entry| normalize_inventory_path_lossy(&entry.path) == requested)
        .map(|entry| entry.classification))
}

fn normalize_inventory_path(path: &Path) -> Result<String> {
    let current_dir = std::env::current_dir().context("failed to resolve current_dir")?;
    Ok(path
        .strip_prefix(&current_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn normalize_inventory_path_lossy(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl RunConfig {
    #[must_use]
    pub fn actor_bindings(&self) -> Vec<ScenarioActorBinding> {
        self.instances
            .iter()
            .map(|instance| ScenarioActorBinding {
                actor: ActorId(instance.id.clone()),
                mode: instance.mode.clone(),
            })
            .collect()
    }

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

        if self.run.runtime_substrate == RuntimeSubstrate::Simulator
            && self
                .instances
                .iter()
                .any(|instance| !matches!(instance.mode, InstanceMode::Local))
        {
            bail!("run.runtime_substrate = \"simulator\" currently supports local instances only");
        }

        let mut instance_ids = HashSet::new();
        let mut local_data_dirs = HashSet::new();
        let mut local_demo_dirs = HashSet::new();
        let mut reserved_bind_addresses = HashSet::new();

        for instance in &self.instances {
            if instance.id.trim().is_empty() {
                bail!("instance id must be non-empty");
            }
            if ActorId(instance.id.clone()).is_frontend_binding_label() {
                bail!(
                    "instance id '{}' is a frontend binding label; run configs must bind frontend modes through instance.mode, not instance.id",
                    instance.id
                );
            }
            if !instance_ids.insert(instance.id.clone()) {
                bail!("duplicate instance id: {}", instance.id);
            }
            if instance.bind_address.trim().is_empty() {
                bail!("instance {} has empty bind_address", instance.id);
            }
            if bind_address_has_explicit_port(&instance.bind_address)?
                && !reserved_bind_addresses.insert(instance.bind_address.trim().to_string())
            {
                bail!(
                    "duplicate explicit bind_address {} for instance {}",
                    instance.bind_address,
                    instance.id
                );
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
                InstanceMode::Browser => {
                    if instance
                        .command
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(str::is_empty)
                    {
                        bail!("browser instance {} has empty command", instance.id);
                    }
                    if instance.ssh_host.is_some()
                        || instance.ssh_user.is_some()
                        || instance.ssh_port.is_some()
                        || instance.remote_workdir.is_some()
                        || instance.tunnel.is_some()
                    {
                        bail!(
                            "browser instance {} must not set ssh_host/ssh_user/ssh_port/remote_workdir/tunnel",
                            instance.id
                        );
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
                    if instance.command.is_some() || !instance.args.is_empty() {
                        bail!(
                            "ssh instance {} must not set local command/args",
                            instance.id
                        );
                    }
                    if instance
                        .env
                        .iter()
                        .any(|entry| !is_harness_env_entry(entry))
                    {
                        bail!(
                            "ssh instance {} must not set non-harness env entries",
                            instance.id
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn bind_address_has_explicit_port(bind_address: &str) -> Result<bool> {
    let (_, port) = bind_address
        .trim()
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("bind_address must be in host:port form, got {bind_address}"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| anyhow!("invalid bind_address port in {bind_address}: {error}"))?;
    Ok(port != 0)
}

impl ScenarioConfig {
    pub fn is_semantic_scenario(&self) -> bool {
        self.compatibility_steps.is_empty() && !self.semantic_steps.is_empty()
    }

    pub fn semantic_steps(&self) -> Option<&[SemanticStep]> {
        self.is_semantic_scenario()
            .then_some(self.semantic_steps.as_slice())
    }

    pub fn compatibility_steps(&self) -> Option<&[CompatibilityStep]> {
        (!self.is_semantic_scenario()).then_some(self.compatibility_steps.as_slice())
    }

    #[must_use]
    pub const fn classification(&self) -> Option<ScenarioClassification> {
        self.classification
    }

    #[must_use]
    pub fn is_frontend_conformance_semantic(&self) -> bool {
        self.is_semantic_scenario()
            && self
                .classification
                .is_some_and(ScenarioClassification::is_frontend_conformance)
    }

    #[must_use]
    pub fn is_shared_semantic(&self) -> bool {
        self.is_semantic_scenario() && !self.is_frontend_conformance_semantic()
    }

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
            if mode != "compatibility" && mode != "agent" {
                bail!("scenario execution_mode must be one of: compatibility, agent");
            }
        }
        if self.compatibility_steps.is_empty() && self.semantic_steps.is_empty() {
            bail!("scenario must include at least one step");
        }
        if !self.compatibility_steps.is_empty() && !self.semantic_steps.is_empty() {
            bail!(
                "scenario {} must not mix compatibility steps with semantic steps",
                self.id
            );
        }

        if self.is_semantic_scenario() {
            if self.semantic_steps.is_empty() {
                bail!("semantic scenario must include at least one semantic step");
            }
            if self.execution_mode.is_some() {
                bail!(
                    "semantic scenario {} must not declare execution_mode; execution follows the semantic lane",
                    self.id
                );
            }
            if !self.compatibility_steps.is_empty() {
                bail!(
                    "semantic scenario {} must not carry mirrored frontend-conformance execution steps",
                    self.id
                );
            }
            let mut step_ids = HashSet::new();
            for step in &self.semantic_steps {
                if step.id.trim().is_empty() {
                    bail!("scenario step id must be non-empty");
                }
                if !step_ids.insert(step.id.clone()) {
                    bail!("duplicate scenario step id: {}", step.id);
                }
            }
            if self.is_shared_semantic()
                && self.semantic_steps.iter().any(|step| {
                    matches!(step.action, SemanticAction::Ui(_))
                        || matches!(
                            step.action,
                            SemanticAction::Expect(Expectation::DiagnosticScreenContains { .. })
                                | SemanticAction::Variables(VariableAction::Extract { .. })
                        )
                })
            {
                bail!(
                    "shared semantic scenario {} contains frontend-conformance-only mechanics or diagnostic observation; classify it as frontend_conformance instead",
                    self.id
                );
            }
            return Ok(());
        }

        if self.execution_mode.is_none() {
            bail!(
                "compatibility scenario {} must declare execution_mode = \"compatibility\" or \"agent\"",
                self.id
            );
        }

        let Some(steps) = self.compatibility_steps() else {
            bail!("validated non-semantic scenarios must expose compatibility steps");
        };
        let mut step_ids = HashSet::new();
        for step in steps {
            if step.id.trim().is_empty() {
                bail!("scenario step id must be non-empty");
            }
            if !step_ids.insert(step.id.clone()) {
                bail!("duplicate scenario step id: {}", step.id);
            }
            if matches!(step.action, CompatibilityAction::WaitFor)
                && step.pattern.is_none()
                && step.selector.is_none()
                && step.contains.is_none()
                && step.level.is_none()
                && step.screen_id.is_none()
                && step.control_id.is_none()
                && step.modal_id.is_none()
                && step.readiness.is_none()
                && step.runtime_event_kind.is_none()
                && step.list_id.is_none()
                && step.operation_id.is_none()
            {
                bail!(
                    "scenario step {} uses wait_for without a semantic target",
                    step.id
                );
            }
            if matches!(step.action, CompatibilityAction::AssertParity)
                && (step.instance.is_none() || step.peer_instance.is_none())
            {
                bail!(
                    "scenario step {} uses assert_parity without both instance and peer_instance",
                    step.id
                );
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
    use aura_app::scenario_contract::{
        ActorId, EnvironmentAction, Expectation, ScenarioAction as SemanticAction, ScenarioStep,
        UiAction,
    };
    use aura_app::ui::contract::{ScreenId, ToastKind, UiReadiness};

    fn semantic_scenario(
        id: &str,
        goal: &str,
        semantic_steps: Vec<ScenarioStep>,
    ) -> ScenarioConfig {
        ScenarioConfig::try_from(ScenarioDefinition {
            id: id.to_string(),
            goal: goal.to_string(),
            steps: semantic_steps,
        })
        .unwrap_or_else(|error| panic!("semantic scenario conversion failed: {error}"))
    }

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
            data_dir = "artifacts/harness/state/test/alice"
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
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![
                InstanceConfig {
                    id: "alice".to_string(),
                    mode: InstanceMode::Local,
                    data_dir: PathBuf::from("artifacts/harness/state/test/shared"),
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
                    data_dir: PathBuf::from("artifacts/harness/state/test/shared"),
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
        let mut config = semantic_scenario("smoke", "test", vec![]);
        config.execution_mode = None;

        let error = match config.validate() {
            Ok(()) => panic!("empty steps must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("at least one step"));
    }

    #[test]
    fn semantic_scenario_file_loads_from_toml() {
        let body = r#"
            id = "semantic-file"
            goal = "semantic file parsing"

            [[steps]]
            id = "nav"
            action = "navigate"
            screen_id = "chat"

            [[steps]]
            id = "toast"
            action = "toast_contains"
            kind = "success"
            value = "done"

            [[steps]]
            id = "diagnostic"
            action = "diagnostic_screen_contains"
            value = "Can enter:"
        "#;

        let file: SemanticScenarioFile = toml::from_str(body)
            .unwrap_or_else(|error| panic!("semantic file parse failed: {error}"));
        let definition = ScenarioDefinition::try_from(file)
            .unwrap_or_else(|error| panic!("semantic file conversion failed: {error}"));
        assert_eq!(definition.steps.len(), 3);
        assert!(matches!(
            definition.steps[0].action,
            SemanticAction::Ui(UiAction::Navigate(ScreenId::Chat))
        ));
        assert!(matches!(
            &definition.steps[1].action,
            SemanticAction::Expect(Expectation::ToastContains {
                kind: Some(ToastKind::Success),
                message_contains
            }) if message_contains == "done"
        ));
        assert!(matches!(
            &definition.steps[2].action,
            SemanticAction::Expect(Expectation::DiagnosticScreenContains { text_contains })
                if text_contains == "Can enter:"
        ));
    }

    #[test]
    fn semantic_definition_translates_into_execution_scenario() {
        let definition = ScenarioDefinition {
            id: "semantic-exec".to_string(),
            goal: "semantic execution translation".to_string(),
            steps: vec![
                SemanticStep {
                    id: "launch".to_string(),
                    actor: None,
                    timeout_ms: Some(1000),
                    action: SemanticAction::Environment(EnvironmentAction::LaunchActors),
                },
                SemanticStep {
                    id: "nav".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(500),
                    action: SemanticAction::Ui(UiAction::Navigate(ScreenId::Chat)),
                },
            ],
        };

        let scenario = ScenarioConfig::try_from(definition)
            .unwrap_or_else(|error| panic!("semantic scenario translation failed: {error}"));

        assert!(scenario.compatibility_steps.is_empty());
        assert_eq!(scenario.execution_mode, None);
        assert_eq!(scenario.semantic_steps.len(), 2);
        assert!(matches!(
            scenario.semantic_steps[0].action,
            SemanticAction::Environment(EnvironmentAction::LaunchActors)
        ));
        assert!(matches!(
            scenario.semantic_steps[1].action,
            SemanticAction::Ui(UiAction::Navigate(ScreenId::Chat))
        ));
    }

    #[test]
    fn semantic_parity_expectation_translates_into_execution_scenario() {
        let definition = ScenarioDefinition {
            id: "semantic-parity".to_string(),
            goal: "semantic parity translation".to_string(),
            steps: vec![SemanticStep {
                id: "parity".to_string(),
                actor: Some(ActorId("web".to_string())),
                timeout_ms: Some(500),
                action: SemanticAction::Expect(Expectation::ParityWithActor {
                    actor: ActorId("tui".to_string()),
                }),
            }],
        };

        let scenario = ScenarioConfig::try_from(definition)
            .unwrap_or_else(|error| panic!("semantic scenario translation failed: {error}"));

        assert!(scenario.compatibility_steps.is_empty());
        assert!(matches!(
            &scenario.semantic_steps[0].action,
            SemanticAction::Expect(Expectation::ParityWithActor { actor })
                if actor.0 == "tui"
        ));
        assert_eq!(
            scenario.semantic_steps[0]
                .actor
                .as_ref()
                .map(|actor| actor.0.as_str()),
            Some("web")
        );
    }

    #[test]
    fn browser_instance_rejects_ssh_fields() {
        let config = RunConfig {
            schema_version: RUN_SCHEMA_VERSION,
            run: RunSection {
                name: "browser-invalid".to_string(),
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
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Browser,
                data_dir: PathBuf::from(".tmp/browser/alice"),
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
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            }],
        };

        let error = match config.validate() {
            Ok(()) => panic!("browser instance must reject ssh fields"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("must not set ssh_host"));
    }

    #[test]
    fn duplicate_explicit_bind_addresses_are_rejected() {
        let config = RunConfig {
            schema_version: RUN_SCHEMA_VERSION,
            run: RunSection {
                name: "duplicate-bind".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(1),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![
                InstanceConfig {
                    id: "alice".to_string(),
                    mode: InstanceMode::Local,
                    data_dir: PathBuf::from(".tmp/alice"),
                    device_id: None,
                    bind_address: "127.0.0.1:41001".to_string(),
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
                    data_dir: PathBuf::from(".tmp/bob"),
                    device_id: None,
                    bind_address: "127.0.0.1:41001".to_string(),
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
        };

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("duplicate explicit bind addresses must fail"));
        assert!(error
            .to_string()
            .contains("duplicate explicit bind_address"));
    }

    #[test]
    fn simulator_substrate_rejects_browser_instances() {
        let config = RunConfig {
            schema_version: RUN_SCHEMA_VERSION,
            run: RunSection {
                name: "simulator-browser-invalid".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(1),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: RuntimeSubstrate::Simulator,
            },
            instances: vec![InstanceConfig {
                id: "browser".to_string(),
                mode: InstanceMode::Browser,
                data_dir: PathBuf::from(".tmp/browser"),
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
            }],
        };

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("simulator substrate should reject browser instances"))
            .to_string();
        assert!(error.contains("currently supports local instances only"));
    }

    #[test]
    fn semantic_scenarios_keep_canonical_steps_directly() {
        let definition = ScenarioDefinition {
            id: "semantic-shared".to_string(),
            goal: "semantic execution".to_string(),
            steps: vec![ScenarioStep {
                id: "launch".to_string(),
                actor: None,
                timeout_ms: Some(1000),
                action: SemanticAction::Environment(EnvironmentAction::LaunchActors),
            }],
        };

        let scenario = ScenarioConfig::try_from(definition)
            .unwrap_or_else(|error| panic!("semantic scenario translation failed: {error}"));
        assert!(scenario.compatibility_steps.is_empty());
        assert_eq!(scenario.semantic_steps.len(), 1);
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn semantic_scenario_rejects_empty_step_ids() {
        let config = semantic_scenario(
            "invalid-step-id",
            "reject empty step id",
            vec![ScenarioStep {
                id: String::new(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Expect(Expectation::ReadinessIs(UiReadiness::Ready)),
            }],
        );

        assert!(config.validate().is_err());
    }

    #[test]
    fn shared_semantic_scenario_rejects_diagnostic_screen_expectation() {
        let config = semantic_scenario(
            "shared-reject-diagnostic-screen",
            "reject conformance-only diagnostic screen expectation in shared lane",
            vec![ScenarioStep {
                id: "diagnostic-screen".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Expect(Expectation::DiagnosticScreenContains {
                    text_contains: "Can enter:".to_string(),
                }),
            }],
        );

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| {
                panic!("shared semantic scenario should reject diagnostic screen expectation")
            })
            .to_string();
        assert!(error.contains("frontend-conformance-only mechanics or diagnostic observation"));
    }

    #[test]
    fn shared_semantic_scenario_rejects_diagnostic_extract() {
        let config = semantic_scenario(
            "shared-reject-diagnostic-extract",
            "reject diagnostic extract in shared lane",
            vec![ScenarioStep {
                id: "extract-screen".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Variables(VariableAction::Extract {
                    name: "capture".to_string(),
                    regex: "Access:".to_string(),
                    group: 0,
                    from: aura_app::scenario_contract::ExtractSource::Screen,
                }),
            }],
        );

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("shared semantic scenario should reject diagnostic extract"))
            .to_string();
        assert!(error.contains("frontend-conformance-only mechanics or diagnostic observation"));
    }

    #[test]
    fn scenario_classification_parses_frontend_conformance_variants() {
        let tui: ScenarioClassification = serde_json::from_str("\"tui_conformance\"")
            .unwrap_or_else(|error| panic!("tui_conformance should parse: {error}"));
        let web: ScenarioClassification = serde_json::from_str("\"web_conformance\"")
            .unwrap_or_else(|error| panic!("web_conformance should parse: {error}"));

        assert_eq!(tui, ScenarioClassification::TuiConformance);
        assert_eq!(web, ScenarioClassification::WebConformance);
    }

    #[test]
    fn scenario_classification_helpers_separate_shared_and_frontend_conformance() {
        assert!(ScenarioClassification::Shared.is_shared_semantic());
        assert!(!ScenarioClassification::Shared.is_frontend_conformance());
        assert!(ScenarioClassification::TuiConformance.is_frontend_conformance());
        assert!(ScenarioClassification::WebConformance.is_frontend_conformance());
    }

    #[test]
    fn compatibility_scenarios_reject_scripted_execution_mode() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "compat-reject-scripted".to_string(),
            goal: "reject legacy compatibility mode alias".to_string(),
            classification: None,
            execution_mode: Some("scripted".to_string()),
            required_capabilities: Vec::new(),
            compatibility_steps: vec![crate::config::CompatibilityStep {
                id: "launch".to_string(),
                action: crate::config::CompatibilityAction::LaunchInstances,
                ..Default::default()
            }],
            semantic_steps: Vec::new(),
        };

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("legacy scripted alias should be rejected"))
            .to_string();
        assert!(error.contains("compatibility, agent"));
    }

    #[test]
    fn compatibility_scenarios_require_explicit_execution_mode() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "compat-require-mode".to_string(),
            goal: "reject implicit compatibility execution mode".to_string(),
            classification: None,
            execution_mode: None,
            required_capabilities: Vec::new(),
            compatibility_steps: vec![crate::config::CompatibilityStep {
                id: "launch".to_string(),
                action: crate::config::CompatibilityAction::LaunchInstances,
                ..Default::default()
            }],
            semantic_steps: Vec::new(),
        };

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("compatibility mode must be explicit"))
            .to_string();
        assert!(error.contains("must declare execution_mode"));
    }

    #[test]
    fn semantic_scenarios_reject_execution_mode() {
        let mut config = semantic_scenario(
            "semantic-reject-mode",
            "reject execution mode on semantic scenario",
            vec![ScenarioStep {
                id: "launch".to_string(),
                actor: None,
                timeout_ms: Some(1000),
                action: SemanticAction::Environment(EnvironmentAction::LaunchActors),
            }],
        );
        config.execution_mode = Some("compatibility".to_string());

        let error = config
            .validate()
            .err()
            .unwrap_or_else(|| panic!("semantic scenarios must reject execution_mode"))
            .to_string();
        assert!(error.contains("must not declare execution_mode"));
    }
}

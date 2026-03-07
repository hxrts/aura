//! Configuration types for harness runs and test scenarios.
//!
//! Defines the schema for run configurations (instances, budgets, resource limits)
//! and scenario definitions (steps, assertions, timeouts) loaded from TOML files.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use aura_app::scenario_contract::{
    ActorId, EnvironmentAction, Expectation, ExtractSource, InputKey,
    ScenarioAction as SemanticAction, ScenarioDefinition, ScenarioStep as SemanticStep,
    SemanticScenarioFile, UiAction, VariableAction,
};
use aura_app::ui::contract::{ControlId, FieldId, ListId, ModalId, ScreenId, ToastKind};
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
    Browser,
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioAction {
    LaunchInstances,
    #[default]
    Noop,
    SetVar,
    ExtractVar,
    SendKeys,
    SendChatCommand,
    SendClipboard,
    ReadClipboard,
    SendKey,
    ClickButton,
    FillInput,
    WaitFor,
    ExpectToast,
    ExpectCommandResult,
    ExpectMembership,
    ExpectDenied,
    GetAuthorityId,
    ListChannels,
    CurrentSelection,
    ListContacts,
    SelectChannel,
    Restart,
    Kill,
    FaultDelay,
    FaultLoss,
    FaultTunnelDrop,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenSource {
    #[default]
    Default,
    Dom,
}

impl fmt::Display for ScenarioAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::LaunchInstances => "launch_instances",
            Self::Noop => "noop",
            Self::SetVar => "set_var",
            Self::ExtractVar => "extract_var",
            Self::SendKeys => "send_keys",
            Self::SendChatCommand => "send_chat_command",
            Self::SendClipboard => "send_clipboard",
            Self::ReadClipboard => "read_clipboard",
            Self::SendKey => "send_key",
            Self::ClickButton => "click_button",
            Self::FillInput => "fill_input",
            Self::WaitFor => "wait_for",
            Self::ExpectToast => "expect_toast",
            Self::ExpectCommandResult => "expect_command_result",
            Self::ExpectMembership => "expect_membership",
            Self::ExpectDenied => "expect_denied",
            Self::GetAuthorityId => "get_authority_id",
            Self::ListChannels => "list_channels",
            Self::CurrentSelection => "current_selection",
            Self::ListContacts => "list_contacts",
            Self::SelectChannel => "select_channel",
            Self::Restart => "restart",
            Self::Kill => "kill",
            Self::FaultDelay => "fault_delay",
            Self::FaultLoss => "fault_loss",
            Self::FaultTunnelDrop => "fault_tunnel_drop",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioStep {
    pub id: String,
    pub action: ScenarioAction,
    pub instance: Option<String>,
    // Backward-compatible overloaded field used by scripted actions.
    // Prefer action-specific fields (`keys`, `command`, `pattern`, `key`,
    // `source_instance`) in new scenarios to keep intent explicit.
    pub expect: Option<String>,
    pub timeout_ms: Option<u64>,
    /// Optional pipeline request id used by strict scenario execution ordering.
    pub request_id: Option<u64>,
    /// Explicit key stream for `send_keys`.
    pub keys: Option<String>,
    /// Screen observation source for browser-oriented waits/assertions.
    pub screen_source: Option<ScreenSource>,
    /// Explicit slash-command body for `send_chat_command`.
    pub command: Option<String>,
    /// Explicit screen pattern for `wait_for`.
    pub pattern: Option<String>,
    /// Explicit named key for `send_key`.
    pub key: Option<String>,
    /// Visible button label for `click_button` when no selector is supplied.
    pub label: Option<String>,
    /// CSS selector for `fill_input` and optional stable target selector for `click_button`.
    pub selector: Option<String>,
    /// Semantic screen reference for typed scenario actions/expectations.
    pub screen_id: Option<ScreenId>,
    /// Semantic control reference for typed scenario actions/expectations.
    pub control_id: Option<ControlId>,
    /// Semantic field reference for typed scenario actions.
    pub field_id: Option<FieldId>,
    /// Semantic modal reference for typed scenario expectations.
    pub modal_id: Option<ModalId>,
    /// Semantic list reference for typed scenario expectations.
    pub list_id: Option<ListId>,
    /// Stable list item identifier for typed list expectations.
    pub item_id: Option<String>,
    /// Repeat count for `send_key` actions.
    pub repeat: Option<u16>,
    /// Explicit source instance for `send_clipboard`.
    pub source_instance: Option<String>,
    /// Variable identifier for set/extract/introspection actions.
    pub var: Option<String>,
    /// Static or templated value for `set_var`.
    pub value: Option<String>,
    /// Regular expression for `extract_var`.
    pub regex: Option<String>,
    /// Capture group index for `extract_var`.
    pub group: Option<usize>,
    /// Source field for `extract_var`: `screen`, `raw_screen`, `authoritative_screen`, or `normalized_screen`.
    pub from: Option<String>,
    /// Substring expectation for typed assertion actions.
    pub contains: Option<String>,
    /// Toast/assertion level (`success`, `info`, `error`) for typed assertions.
    pub level: Option<String>,
    /// Command outcome status (`ok`, `denied`, `invalid`, `failed`) for command assertions.
    pub status: Option<String>,
    /// Consistency label (`accepted`, `replicated`, `enforced`, `partial-timeout`) for command-result assertions.
    pub consistency: Option<String>,
    /// Stable command reason code (for normalized command outcomes).
    pub reason_code: Option<String>,
    /// Channel display name for membership assertions.
    pub channel: Option<String>,
    /// Expected selected state for membership assertions.
    pub selected: Option<bool>,
    /// Expected present state for membership assertions (defaults to true when omitted).
    pub present: Option<bool>,
    /// Denial reason discriminator for `expect_denied` (`permission`, `banned`, `muted`).
    pub reason: Option<String>,
    /// Additional allowed denial substrings for `expect_denied`.
    pub contains_any: Option<Vec<String>>,
}

impl ScenarioStep {
    pub fn to_semantic_step(&self) -> Result<Option<SemanticStep>> {
        let actor = self.instance.clone().map(ActorId);
        let timeout_ms = self.timeout_ms;
        let id = self.id.clone();

        let action = match self.action {
            ScenarioAction::LaunchInstances => {
                Some(SemanticAction::Environment(EnvironmentAction::LaunchActors))
            }
            ScenarioAction::Noop => None,
            ScenarioAction::SetVar => Some(SemanticAction::Variables(VariableAction::Set {
                name: required_field(self.var.clone(), "var", self.action)?,
                value: required_field(
                    self.value.clone().or_else(|| self.expect.clone()),
                    "value",
                    self.action,
                )?,
            })),
            ScenarioAction::ExtractVar => {
                Some(SemanticAction::Variables(VariableAction::Extract {
                    name: required_field(self.var.clone(), "var", self.action)?,
                    regex: required_field(self.regex.clone(), "regex", self.action)?,
                    group: self.group.unwrap_or(0),
                    from: parse_extract_source(self.from.as_deref().unwrap_or("screen"))?,
                }))
            }
            ScenarioAction::SendKeys => {
                Some(SemanticAction::Ui(UiAction::InputText(required_field(
                    self.keys.clone().or_else(|| self.expect.clone()),
                    "keys",
                    self.action,
                )?)))
            }
            ScenarioAction::SendClipboard => Some(SemanticAction::Ui(UiAction::PasteClipboard {
                source_actor: self.source_instance.clone().map(ActorId),
            })),
            ScenarioAction::ReadClipboard => Some(SemanticAction::Ui(UiAction::ReadClipboard)),
            ScenarioAction::SendKey => Some(SemanticAction::Ui(UiAction::PressKey(
                parse_input_key(self.key.as_deref().unwrap_or_default())?,
                self.repeat.unwrap_or(1).max(1),
            ))),
            ScenarioAction::ClickButton => match self.control_id {
                Some(control_id) => Some(SemanticAction::Ui(UiAction::Activate(control_id))),
                None => None,
            },
            ScenarioAction::FillInput => match self.field_id {
                Some(field_id) => Some(SemanticAction::Ui(UiAction::Fill(
                    field_id,
                    required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                ))),
                None => None,
            },
            ScenarioAction::WaitFor => expectation_from_step(self)?,
            ScenarioAction::ExpectToast => {
                Some(SemanticAction::Expect(Expectation::ToastContains {
                    kind: self.level.as_deref().map(parse_toast_kind).transpose()?,
                    message_contains: required_field(
                        self.contains.clone().or_else(|| self.expect.clone()),
                        "contains",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::Restart => Some(SemanticAction::Environment(
                EnvironmentAction::RestartActor {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                },
            )),
            ScenarioAction::Kill => {
                Some(SemanticAction::Environment(EnvironmentAction::KillActor {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                }))
            }
            ScenarioAction::FaultDelay => {
                Some(SemanticAction::Environment(EnvironmentAction::FaultDelay {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                    delay_ms: self.timeout_ms.unwrap_or_default(),
                }))
            }
            ScenarioAction::FaultLoss => {
                Some(SemanticAction::Environment(EnvironmentAction::FaultLoss {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                    loss_percent: 100,
                }))
            }
            ScenarioAction::FaultTunnelDrop => Some(SemanticAction::Environment(
                EnvironmentAction::FaultTunnelDrop {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                },
            )),
            ScenarioAction::SendChatCommand
            | ScenarioAction::ExpectCommandResult
            | ScenarioAction::ExpectMembership
            | ScenarioAction::ExpectDenied
            | ScenarioAction::GetAuthorityId
            | ScenarioAction::ListChannels
            | ScenarioAction::CurrentSelection
            | ScenarioAction::ListContacts
            | ScenarioAction::SelectChannel => None,
        };

        Ok(action.map(|action| SemanticStep {
            id,
            actor,
            timeout_ms,
            action,
        }))
    }
}

fn required_field(value: Option<String>, field: &str, action: ScenarioAction) -> Result<String> {
    value.ok_or_else(|| anyhow!("action {} requires {field}", action))
}

fn parse_extract_source(value: &str) -> Result<ExtractSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "screen" => Ok(ExtractSource::Screen),
        "raw_screen" => Ok(ExtractSource::RawScreen),
        "authoritative_screen" => Ok(ExtractSource::AuthoritativeScreen),
        "normalized_screen" => Ok(ExtractSource::NormalizedScreen),
        other => bail!("unsupported extract source: {other}"),
    }
}

fn parse_input_key(value: &str) -> Result<InputKey> {
    match value.trim().to_ascii_lowercase().as_str() {
        "enter" => Ok(InputKey::Enter),
        "esc" => Ok(InputKey::Esc),
        "tab" => Ok(InputKey::Tab),
        "backtab" | "back_tab" | "shift_tab" => Ok(InputKey::BackTab),
        "up" => Ok(InputKey::Up),
        "down" => Ok(InputKey::Down),
        "left" => Ok(InputKey::Left),
        "right" => Ok(InputKey::Right),
        "home" => Ok(InputKey::Home),
        "end" => Ok(InputKey::End),
        "pageup" | "page_up" => Ok(InputKey::PageUp),
        "pagedown" | "page_down" => Ok(InputKey::PageDown),
        "backspace" => Ok(InputKey::Backspace),
        "delete" => Ok(InputKey::Delete),
        other => bail!("unsupported semantic key: {other}"),
    }
}

fn parse_toast_kind(value: &str) -> Result<ToastKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "success" => Ok(ToastKind::Success),
        "info" => Ok(ToastKind::Info),
        "error" => Ok(ToastKind::Error),
        other => bail!("unsupported toast kind: {other}"),
    }
}

fn expectation_from_step(step: &ScenarioStep) -> Result<Option<SemanticAction>> {
    if let Some(screen_id) = step.screen_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ScreenIs(
            screen_id,
        ))));
    }
    if let Some(control_id) = step.control_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ControlVisible(
            control_id,
        ))));
    }
    if let Some(modal_id) = step.modal_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ModalOpen(
            modal_id,
        ))));
    }
    if let (Some(list_id), Some(item_id)) = (step.list_id, step.item_id.clone()) {
        return Ok(Some(SemanticAction::Expect(Expectation::ListContains {
            list: list_id,
            item_id,
        })));
    }
    Ok(None)
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
    ScenarioDefinition::try_from(file).map_err(|error| {
        anyhow!(
            "failed to convert semantic scenario at {}: {error}",
            path.display()
        )
    })
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
            if step.request_id == Some(0) {
                bail!("scenario step {} request_id must be >= 1", step.id);
            }
        }

        Ok(())
    }

    pub fn to_semantic_definition(&self) -> Result<ScenarioDefinition> {
        let mut steps = Vec::new();
        for step in &self.steps {
            if let Some(semantic_step) = step.to_semantic_step()? {
                steps.push(semantic_step);
            }
        }
        Ok(ScenarioDefinition {
            id: self.id.clone(),
            goal: self.goal.clone(),
            steps,
        })
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
    fn scenario_step_explicit_fields_parse_from_toml() {
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
        assert_eq!(parsed.steps[0].keys.as_deref(), Some("hello\n"));
        assert_eq!(parsed.steps[1].command.as_deref(), Some("join slash-lab"));
        assert_eq!(parsed.steps[2].pattern.as_deref(), Some("slash-lab"));
        assert_eq!(parsed.steps[3].key.as_deref(), Some("esc"));
        assert_eq!(parsed.steps[4].source_instance.as_deref(), Some("alice"));
    }

    #[test]
    fn scenario_step_unknown_action_is_rejected_during_parse() {
        let body = r#"
            schema_version = 1
            id = "invalid-action"
            goal = "invalid action should fail parsing"
            execution_mode = "scripted"
            required_capabilities = []

            [[steps]]
            id = "bad"
            action = "not_a_real_action"
        "#;

        let parsed: Result<ScenarioConfig, _> = toml::from_str(body);
        assert!(parsed.is_err());
    }

    #[test]
    fn semantic_translation_maps_typed_wait_and_fill_steps() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "semantic".to_string(),
            goal: "semantic translation".to_string(),
            required_capabilities: vec![],
            execution_mode: Some("scripted".to_string()),
            steps: vec![
                ScenarioStep {
                    id: "fill-nickname".to_string(),
                    action: ScenarioAction::FillInput,
                    instance: Some("web".to_string()),
                    field_id: Some(FieldId::Nickname),
                    value: Some("Ops".to_string()),
                    ..ScenarioStep::default()
                },
                ScenarioStep {
                    id: "wait-settings".to_string(),
                    action: ScenarioAction::WaitFor,
                    instance: Some("web".to_string()),
                    screen_id: Some(ScreenId::Settings),
                    ..ScenarioStep::default()
                },
            ],
        };

        let semantic = config
            .to_semantic_definition()
            .unwrap_or_else(|error| panic!("semantic translation failed: {error}"));
        assert_eq!(semantic.id, "semantic");
        assert_eq!(semantic.steps.len(), 2);
        assert!(matches!(
            semantic.steps[0].action,
            SemanticAction::Ui(UiAction::Fill(FieldId::Nickname, ref value)) if value == "Ops"
        ));
        assert!(matches!(
            semantic.steps[1].action,
            SemanticAction::Expect(Expectation::ScreenIs(ScreenId::Settings))
        ));
    }

    #[test]
    fn semantic_translation_rejects_missing_required_fields() {
        let step = ScenarioStep {
            id: "bad-fill".to_string(),
            action: ScenarioAction::FillInput,
            instance: Some("web".to_string()),
            field_id: Some(FieldId::Nickname),
            ..ScenarioStep::default()
        };

        let error = step
            .to_semantic_step()
            .expect_err("fill input without value must fail");
        assert!(error
            .to_string()
            .contains("action fill_input requires value"));
    }

    #[test]
    fn semantic_translation_maps_toast_expectation_kind() {
        let step = ScenarioStep {
            id: "toast".to_string(),
            action: ScenarioAction::ExpectToast,
            level: Some("error".to_string()),
            contains: Some("denied".to_string()),
            ..ScenarioStep::default()
        };

        let semantic = step
            .to_semantic_step()
            .unwrap_or_else(|error| panic!("toast semantic translation failed: {error}"))
            .expect("toast step should map");

        assert!(matches!(
            semantic.action,
            SemanticAction::Expect(Expectation::ToastContains {
                kind: Some(ToastKind::Error),
                message_contains
            }) if message_contains == "denied"
        ));
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
        "#;

        let file: SemanticScenarioFile = toml::from_str(body)
            .unwrap_or_else(|error| panic!("semantic file parse failed: {error}"));
        let definition = ScenarioDefinition::try_from(file)
            .unwrap_or_else(|error| panic!("semantic file conversion failed: {error}"));
        assert_eq!(definition.steps.len(), 2);
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
}

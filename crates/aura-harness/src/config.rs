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
use aura_app::ui::contract::{
    ConfirmationState, ControlId, FieldId, ListId, ModalId, OperationId, OperationState,
    ScreenId, ToastKind, UiReadiness,
};
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
    CaptureCurrentAuthorityId,
    CaptureSelection,
    ExtractVar,
    SendKeys,
    SendChatCommand,
    SendChatMessage,
    SendClipboard,
    ReadClipboard,
    DismissTransient,
    SendKey,
    ClickButton,
    FillInput,
    WaitFor,
    MessageContains,
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
            Self::CaptureCurrentAuthorityId => "capture_current_authority_id",
            Self::CaptureSelection => "capture_selection",
            Self::ExtractVar => "extract_var",
            Self::SendKeys => "send_keys",
            Self::SendChatCommand => "send_chat_command",
            Self::SendChatMessage => "send_chat_message",
            Self::SendClipboard => "send_clipboard",
            Self::ReadClipboard => "read_clipboard",
            Self::DismissTransient => "dismiss_transient",
            Self::SendKey => "send_key",
            Self::ClickButton => "click_button",
            Self::FillInput => "fill_input",
            Self::WaitFor => "wait_for",
            Self::MessageContains => "message_contains",
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
    /// Semantic readiness reference for typed scenario expectations.
    pub readiness: Option<UiReadiness>,
    /// Semantic operation identifier for typed lifecycle expectations.
    pub operation_id: Option<OperationId>,
    /// Semantic operation state for typed lifecycle expectations.
    pub operation_state: Option<OperationState>,
    /// Semantic list reference for typed scenario expectations.
    pub list_id: Option<ListId>,
    /// Stable list item identifier for typed list expectations.
    pub item_id: Option<String>,
    /// Expected list size for typed list expectations.
    pub count: Option<usize>,
    /// Expected confirmation state for typed list expectations.
    pub confirmation: Option<ConfirmationState>,
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
            ScenarioAction::CaptureCurrentAuthorityId => {
                Some(SemanticAction::Variables(
                    VariableAction::CaptureCurrentAuthorityId {
                        name: required_field(self.var.clone(), "var", self.action)?,
                    },
                ))
            }
            ScenarioAction::CaptureSelection => Some(SemanticAction::Variables(
                VariableAction::CaptureSelection {
                    name: required_field(self.var.clone(), "var", self.action)?,
                    list: self
                        .list_id
                        .ok_or_else(|| anyhow!("action {} requires list_id", self.action))?,
                },
            )),
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
            ScenarioAction::ReadClipboard => {
                Some(SemanticAction::Ui(UiAction::ReadClipboard {
                    name: required_field(self.var.clone(), "var", self.action)?,
                }))
            }
            ScenarioAction::DismissTransient => Some(SemanticAction::Ui(
                UiAction::DismissTransient,
            )),
            ScenarioAction::SendKey => Some(SemanticAction::Ui(UiAction::PressKey(
                parse_input_key(self.key.as_deref().unwrap_or_default())?,
                self.repeat.unwrap_or(1).max(1),
            ))),
            ScenarioAction::SendChatCommand => Some(SemanticAction::Ui(UiAction::SendChatCommand(
                required_field(
                    self.command.clone().or_else(|| self.expect.clone()),
                    "command",
                    self.action,
                )?,
            ))),
            ScenarioAction::SendChatMessage => Some(SemanticAction::Ui(UiAction::SendChatMessage(
                required_field(
                    self.value.clone().or_else(|| self.expect.clone()),
                    "value",
                    self.action,
                )?,
            ))),
            ScenarioAction::ClickButton => {
                if let Some(control_id) = self.control_id {
                    Some(SemanticAction::Ui(UiAction::Activate(control_id)))
                } else if let (Some(list_id), Some(item_id)) =
                    (self.list_id, self.item_id.clone())
                {
                    Some(SemanticAction::Ui(UiAction::ActivateListItem {
                        list: list_id,
                        item_id,
                    }))
                } else {
                    None
                }
            }
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
            ScenarioAction::MessageContains => {
                Some(SemanticAction::Expect(Expectation::MessageContains {
                    message_contains: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
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
            ScenarioAction::ExpectCommandResult
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

impl TryFrom<ScenarioDefinition> for ScenarioConfig {
    type Error = anyhow::Error;

    fn try_from(value: ScenarioDefinition) -> Result<Self> {
        let mut steps = Vec::with_capacity(value.steps.len());
        for step in value.steps {
            steps.push(ScenarioStep::try_from(step)?);
        }
        Ok(Self {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: value.id,
            goal: value.goal,
            execution_mode: Some("scripted".to_string()),
            required_capabilities: Vec::new(),
            steps,
        })
    }
}

impl TryFrom<SemanticStep> for ScenarioStep {
    type Error = anyhow::Error;

    fn try_from(value: SemanticStep) -> Result<Self> {
        let SemanticStep {
            id,
            actor,
            timeout_ms,
            action,
        } = value;

        let instance = actor.map(|actor| actor.0);
        let mut step = ScenarioStep {
            id,
            instance,
            timeout_ms,
            ..Default::default()
        };

        match action {
            SemanticAction::Environment(EnvironmentAction::LaunchActors) => {
                step.action = ScenarioAction::LaunchInstances;
            }
            SemanticAction::Environment(EnvironmentAction::RestartActor { actor }) => {
                step.action = ScenarioAction::Restart;
                step.instance = Some(actor.0);
            }
            SemanticAction::Environment(EnvironmentAction::KillActor { actor }) => {
                step.action = ScenarioAction::Kill;
                step.instance = Some(actor.0);
            }
            SemanticAction::Environment(EnvironmentAction::FaultDelay { actor, delay_ms }) => {
                step.action = ScenarioAction::FaultDelay;
                step.instance = Some(actor.0);
                step.timeout_ms = Some(delay_ms);
            }
            SemanticAction::Environment(EnvironmentAction::FaultLoss {
                actor,
                loss_percent,
            }) => {
                step.action = ScenarioAction::FaultLoss;
                step.instance = Some(actor.0);
                step.expect = Some(loss_percent.to_string());
            }
            SemanticAction::Environment(EnvironmentAction::FaultTunnelDrop { actor }) => {
                step.action = ScenarioAction::FaultTunnelDrop;
                step.instance = Some(actor.0);
            }
            SemanticAction::Ui(UiAction::Navigate(screen_id)) => {
                step.action = ScenarioAction::ClickButton;
                step.control_id = Some(nav_control_id_for_screen(screen_id));
            }
            SemanticAction::Ui(UiAction::Activate(control_id)) => {
                step.action = ScenarioAction::ClickButton;
                step.control_id = Some(control_id);
            }
            SemanticAction::Ui(UiAction::ActivateListItem { list, item_id }) => {
                step.action = ScenarioAction::ClickButton;
                step.list_id = Some(list);
                step.item_id = Some(item_id);
            }
            SemanticAction::Ui(UiAction::Fill(field_id, value)) => {
                step.action = ScenarioAction::FillInput;
                step.field_id = Some(field_id);
                step.value = Some(value);
            }
            SemanticAction::Ui(UiAction::InputText(value)) => {
                step.action = ScenarioAction::SendKeys;
                step.keys = Some(value);
            }
            SemanticAction::Ui(UiAction::PressKey(key, repeat)) => {
                step.action = ScenarioAction::SendKey;
                step.key = Some(format_input_key(key));
                step.repeat = Some(repeat);
            }
            SemanticAction::Ui(UiAction::SendChatCommand(command)) => {
                step.action = ScenarioAction::SendChatCommand;
                step.command = Some(command);
            }
            SemanticAction::Ui(UiAction::SendChatMessage(message)) => {
                step.action = ScenarioAction::SendChatMessage;
                step.value = Some(message);
            }
            SemanticAction::Ui(UiAction::PasteClipboard { source_actor }) => {
                step.action = ScenarioAction::SendClipboard;
                step.source_instance = source_actor.map(|actor| actor.0);
            }
            SemanticAction::Ui(UiAction::ReadClipboard { name }) => {
                step.action = ScenarioAction::ReadClipboard;
                step.var = Some(name);
            }
            SemanticAction::Ui(UiAction::DismissTransient) => {
                step.action = ScenarioAction::DismissTransient;
            }
            SemanticAction::Expect(Expectation::ScreenIs(screen_id)) => {
                step.action = ScenarioAction::WaitFor;
                step.screen_id = Some(screen_id);
            }
            SemanticAction::Expect(Expectation::ControlVisible(control_id)) => {
                step.action = ScenarioAction::WaitFor;
                step.control_id = Some(control_id);
            }
            SemanticAction::Expect(Expectation::ModalOpen(modal_id)) => {
                step.action = ScenarioAction::WaitFor;
                step.modal_id = Some(modal_id);
            }
            SemanticAction::Expect(Expectation::MessageContains { message_contains }) => {
                step.action = ScenarioAction::MessageContains;
                step.value = Some(message_contains);
            }
            SemanticAction::Expect(Expectation::ToastContains {
                kind,
                message_contains,
            }) => {
                step.action = ScenarioAction::WaitFor;
                step.level = kind.map(format_toast_kind);
                step.contains = Some(message_contains);
            }
            SemanticAction::Expect(Expectation::ListContains { list, item_id }) => {
                step.action = ScenarioAction::WaitFor;
                step.list_id = Some(list);
                step.item_id = Some(item_id);
            }
            SemanticAction::Expect(Expectation::ListCountIs { list, count }) => {
                step.action = ScenarioAction::WaitFor;
                step.list_id = Some(list);
                step.count = Some(count);
            }
            SemanticAction::Expect(Expectation::ListItemConfirmation {
                list,
                item_id,
                confirmation,
            }) => {
                step.action = ScenarioAction::WaitFor;
                step.list_id = Some(list);
                step.item_id = Some(item_id);
                step.confirmation = Some(confirmation);
            }
            SemanticAction::Expect(Expectation::SelectionIs { list, item_id }) => {
                step.action = ScenarioAction::WaitFor;
                step.list_id = Some(list);
                step.item_id = Some(item_id);
            }
            SemanticAction::Expect(Expectation::ReadinessIs(readiness)) => {
                step.action = ScenarioAction::WaitFor;
                step.readiness = Some(readiness);
            }
            SemanticAction::Expect(Expectation::OperationStateIs {
                operation_id,
                state,
            }) => {
                step.action = ScenarioAction::WaitFor;
                step.operation_id = Some(operation_id);
                step.operation_state = Some(state);
            }
            SemanticAction::Variables(VariableAction::Set { name, value }) => {
                step.action = ScenarioAction::SetVar;
                step.var = Some(name);
                step.value = Some(value);
            }
            SemanticAction::Variables(VariableAction::CaptureCurrentAuthorityId { name }) => {
                step.action = ScenarioAction::CaptureCurrentAuthorityId;
                step.var = Some(name);
            }
            SemanticAction::Variables(VariableAction::CaptureSelection { name, list }) => {
                step.action = ScenarioAction::CaptureSelection;
                step.var = Some(name);
                step.list_id = Some(list);
            }
            SemanticAction::Variables(VariableAction::Extract { name, regex, group, from }) => {
                step.action = ScenarioAction::ExtractVar;
                step.var = Some(name);
                step.regex = Some(regex);
                step.group = Some(group);
                step.from = Some(format_extract_source(from));
            }
        }

        Ok(step)
    }
}

fn required_field(value: Option<String>, field: &str, action: ScenarioAction) -> Result<String> {
    value.ok_or_else(|| anyhow!("action {} requires {field}", action))
}

fn nav_control_id_for_screen(screen_id: ScreenId) -> ControlId {
    match screen_id {
        ScreenId::Neighborhood => ControlId::NavNeighborhood,
        ScreenId::Chat => ControlId::NavChat,
        ScreenId::Contacts => ControlId::NavContacts,
        ScreenId::Notifications => ControlId::NavNotifications,
        ScreenId::Settings => ControlId::NavSettings,
    }
}

fn format_input_key(value: InputKey) -> String {
    match value {
        InputKey::Enter => "enter",
        InputKey::Esc => "esc",
        InputKey::Tab => "tab",
        InputKey::BackTab => "backtab",
        InputKey::Up => "up",
        InputKey::Down => "down",
        InputKey::Left => "left",
        InputKey::Right => "right",
        InputKey::Home => "home",
        InputKey::End => "end",
        InputKey::PageUp => "pageup",
        InputKey::PageDown => "pagedown",
        InputKey::Backspace => "backspace",
        InputKey::Delete => "delete",
    }
    .to_string()
}

fn format_toast_kind(value: ToastKind) -> String {
    match value {
        ToastKind::Success => "success",
        ToastKind::Info => "info",
        ToastKind::Error => "error",
    }
    .to_string()
}

fn format_extract_source(value: ExtractSource) -> String {
    match value {
        ExtractSource::Screen => "screen",
        ExtractSource::RawScreen => "raw_screen",
        ExtractSource::AuthoritativeScreen => "authoritative_screen",
        ExtractSource::NormalizedScreen => "normalized_screen",
    }
    .to_string()
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
    if matches!(step.action, ScenarioAction::MessageContains) {
        return Ok(Some(SemanticAction::Expect(Expectation::MessageContains {
            message_contains: required_field(
                step.value.clone().or_else(|| step.expect.clone()),
                "value",
                step.action,
            )?,
        })));
    }
    if step.contains.is_some() || step.level.is_some() {
        return Ok(Some(SemanticAction::Expect(Expectation::ToastContains {
            kind: step.level.as_deref().map(parse_toast_kind).transpose()?,
            message_contains: required_field(
                step.contains.clone().or_else(|| step.expect.clone()),
                "contains",
                step.action,
            )?,
        })));
    }
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
    if let Some(readiness) = step.readiness {
        return Ok(Some(SemanticAction::Expect(Expectation::ReadinessIs(
            readiness,
        ))));
    }
    if let (Some(operation_id), Some(state)) = (step.operation_id.clone(), step.operation_state) {
        return Ok(Some(SemanticAction::Expect(Expectation::OperationStateIs {
            operation_id,
            state,
        })));
    }
    if let (Some(list_id), Some(item_id)) = (step.list_id, step.item_id.clone()) {
        if let Some(confirmation) = step.confirmation {
            return Ok(Some(SemanticAction::Expect(
                Expectation::ListItemConfirmation {
                    list: list_id,
                    item_id,
                    confirmation,
                },
            )));
        }
        return Ok(Some(SemanticAction::Expect(Expectation::ListContains {
            list: list_id,
            item_id,
        })));
    }
    if let (Some(list_id), Some(count)) = (step.list_id, step.count) {
        return Ok(Some(SemanticAction::Expect(Expectation::ListCountIs {
            list: list_id,
            count,
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

pub fn load_execution_scenario_config(path: &Path) -> Result<ScenarioConfig> {
    let semantic = load_semantic_scenario_definition(path)
        .with_context(|| format!("failed to load semantic scenario at {}", path.display()))?;
    ScenarioConfig::try_from(semantic).with_context(|| {
        format!(
            "failed to translate semantic scenario {} into harness execution steps",
            path.display()
        )
    })
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

        if self.run.runtime_substrate == RuntimeSubstrate::Simulator
            && self
                .instances
                .iter()
                .any(|instance| !matches!(instance.mode, InstanceMode::Local))
        {
            bail!(
                "run.runtime_substrate = \"simulator\" currently supports local instances only"
            );
        }

        let mut instance_ids = HashSet::new();
        let mut local_data_dirs = HashSet::new();
        let mut local_demo_dirs = HashSet::new();
        let mut reserved_bind_addresses = HashSet::new();

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
            if matches!(step.action, ScenarioAction::Noop) {
                bail!(
                    "scenario step {} uses noop; use a semantic wait or explicit action instead",
                    step.id
                );
            }
            if matches!(step.action, ScenarioAction::WaitFor)
                && step.pattern.is_none()
                && step.selector.is_none()
                && step.contains.is_none()
                && step.level.is_none()
                && step.screen_id.is_none()
                && step.control_id.is_none()
                && step.modal_id.is_none()
                && step.readiness.is_none()
                && step.list_id.is_none()
                && step.operation_id.is_none()
            {
                bail!(
                    "scenario step {} uses wait_for without a semantic target",
                    step.id
                );
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
                runtime_substrate: Default::default(),
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

        assert_eq!(scenario.steps.len(), 2);
        assert_eq!(scenario.steps[0].action, ScenarioAction::LaunchInstances);
        assert_eq!(scenario.steps[1].action, ScenarioAction::ClickButton);
        assert_eq!(scenario.steps[1].instance.as_deref(), Some("alice"));
        assert_eq!(scenario.steps[1].control_id, Some(ControlId::NavChat));
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
                runtime_substrate: Default::default(),
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
                runtime_substrate: Default::default(),
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
        assert!(error.to_string().contains("duplicate explicit bind_address"));
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
    fn scenario_noop_steps_are_rejected() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "noop-invalid".to_string(),
            goal: "reject noop".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: Vec::new(),
            steps: vec![ScenarioStep {
                id: "noop".to_string(),
                action: ScenarioAction::Noop,
                ..ScenarioStep::default()
            }],
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn scenario_wait_for_requires_target() {
        let config = ScenarioConfig {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "wait-invalid".to_string(),
            goal: "reject bare wait".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: Vec::new(),
            steps: vec![ScenarioStep {
                id: "wait".to_string(),
                action: ScenarioAction::WaitFor,
                timeout_ms: Some(1000),
                ..ScenarioStep::default()
            }],
        };

        assert!(config.validate().is_err());
    }
}

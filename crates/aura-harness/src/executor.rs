use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::config::{ScenarioAction, ScenarioConfig, ScenarioStep};
use crate::introspection::{extract_command_consistency, extract_toast, ToastLevel};
use crate::tool_api::{ToolApi, ToolKey, ToolRequest, ToolResponse};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Scripted,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioReport {
    pub scenario_id: String,
    pub execution_mode: ExecutionMode,
    pub states_visited: Vec<String>,
    pub transitions: Vec<StateTransitionEvent>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateTransitionEvent {
    pub from_state: String,
    pub to_state: Option<String>,
    pub reason: String,
}

pub struct ScenarioExecutor {
    mode: ExecutionMode,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionBudgets {
    pub global_budget_ms: Option<u64>,
    pub default_step_budget_ms: u64,
    pub scenario_seed: u64,
    pub fault_seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenField {
    Screen,
    RawScreen,
    AuthoritativeScreen,
    NormalizedScreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedConsistency {
    Accepted,
    Replicated,
    Enforced,
    PartialTimeout,
}

impl ExpectedConsistency {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "accepted" => Ok(Self::Accepted),
            "replicated" => Ok(Self::Replicated),
            "enforced" => Ok(Self::Enforced),
            "partial-timeout" | "partial_timeout" | "partialtimeout" => Ok(Self::PartialTimeout),
            other => bail!("unsupported consistency value: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeniedReason {
    Permission,
    Banned,
    Muted,
}

impl DeniedReason {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "permission" => Ok(Self::Permission),
            "banned" => Ok(Self::Banned),
            "muted" => Ok(Self::Muted),
            other => bail!("unsupported denied reason: {other}"),
        }
    }

    fn patterns(self) -> &'static [&'static str] {
        match self {
            Self::Permission => &["permission", "denied", "auth"],
            Self::Banned => &["ban", "banned", "denied"],
            Self::Muted => &["mute", "muted", "denied"],
        }
    }
}

#[derive(Debug, Default, Clone)]
struct ScenarioContext {
    vars: BTreeMap<String, String>,
    last_request_id: Option<u64>,
}

impl Default for ExecutionBudgets {
    fn default() -> Self {
        Self {
            global_budget_ms: None,
            default_step_budget_ms: 2000,
            scenario_seed: 0,
            fault_seed: 0,
        }
    }
}

impl ScenarioExecutor {
    pub fn new(mode: ExecutionMode) -> Self {
        Self { mode }
    }

    pub fn from_config(config: &ScenarioConfig) -> Self {
        let mode = match config.execution_mode.as_deref() {
            Some("agent") => ExecutionMode::Agent,
            _ => ExecutionMode::Scripted,
        };
        Self::new(mode)
    }

    pub fn execute(
        &self,
        scenario: &ScenarioConfig,
        tool_api: &mut ToolApi,
    ) -> Result<ScenarioReport> {
        self.execute_with_budgets(scenario, tool_api, ExecutionBudgets::default())
    }

    pub fn execute_with_budgets(
        &self,
        scenario: &ScenarioConfig,
        tool_api: &mut ToolApi,
        budgets: ExecutionBudgets,
    ) -> Result<ScenarioReport> {
        let machine = StateMachine::from_steps(&scenario.steps);
        let mut current = machine
            .start_state
            .clone()
            .ok_or_else(|| anyhow!("scenario has no start state"))?;
        let mut visited = Vec::new();
        let mut transitions = Vec::new();
        let mut global_remaining = budgets.global_budget_ms;
        let mut scenario_rng = DeterministicRng::new(budgets.scenario_seed);
        let mut fault_rng = DeterministicRng::new(budgets.fault_seed);
        let mut context = ScenarioContext::default();

        loop {
            let state = machine
                .states
                .get(&current)
                .ok_or_else(|| anyhow!("missing state {current}"))?;
            let step_budget = state
                .step
                .timeout_ms
                .unwrap_or(budgets.default_step_budget_ms);
            if let Some(remaining) = global_remaining {
                if remaining < step_budget {
                    bail!(
                        "scenario budget exceeded at state {} remaining_ms={} required_ms={}",
                        state.id,
                        remaining,
                        step_budget
                    );
                }
                global_remaining = Some(remaining.saturating_sub(step_budget));
            }
            visited.push(state.id.clone());
            execute_step(
                &state.step,
                tool_api,
                step_budget,
                &mut scenario_rng,
                &mut fault_rng,
                &mut context,
            )?;

            let next = match self.mode {
                ExecutionMode::Scripted => state.next_state.clone(),
                // Agent mode currently reuses the same transition graph and chooses the next
                // valid edge, making behavior deterministic until agent planning is added.
                ExecutionMode::Agent => state.next_state.clone(),
            };

            transitions.push(StateTransitionEvent {
                from_state: state.id.clone(),
                to_state: next.clone(),
                reason: "step_complete".to_string(),
            });

            let Some(next_state) = next else {
                break;
            };
            current = next_state;
        }

        Ok(ScenarioReport {
            scenario_id: scenario.id.clone(),
            execution_mode: self.mode,
            states_visited: visited,
            transitions,
            completed: true,
        })
    }
}

#[derive(Debug, Clone)]
struct ScenarioState {
    id: String,
    step: ScenarioStep,
    next_state: Option<String>,
}

#[derive(Debug, Clone)]
struct StateMachine {
    start_state: Option<String>,
    states: BTreeMap<String, ScenarioState>,
}

impl StateMachine {
    fn from_steps(steps: &[ScenarioStep]) -> Self {
        let mut states = BTreeMap::new();

        for (index, step) in steps.iter().enumerate() {
            let next_state = steps.get(index + 1).map(|step| step.id.clone());
            states.insert(
                step.id.clone(),
                ScenarioState {
                    id: step.id.clone(),
                    step: step.clone(),
                    next_state,
                },
            );
        }

        Self {
            start_state: steps.first().map(|step| step.id.clone()),
            states,
        }
    }
}

fn execute_step(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    scenario_rng: &mut DeterministicRng,
    fault_rng: &mut DeterministicRng,
    context: &mut ScenarioContext,
) -> Result<()> {
    enforce_request_order(step, context)?;
    match step.action {
        ScenarioAction::LaunchInstances | ScenarioAction::Noop => Ok(()),
        ScenarioAction::SetVar => {
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let raw_value = step
                .value
                .as_deref()
                .or(step.expect.as_deref())
                .ok_or_else(|| anyhow!("step {} missing value", step.id))?;
            let value = resolve_template(raw_value, context)?;
            context.vars.insert(var.to_string(), value);
            Ok(())
        }
        ScenarioAction::ExtractVar => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let regex_pattern = resolve_required_field(
                step,
                "regex",
                step.regex.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let field = parse_screen_field(step.from.as_deref().unwrap_or("screen"))?;
            let payload = dispatch_payload(
                tool_api,
                ToolRequest::Screen {
                    instance_id,
                },
            )?;
            let source = screen_field_value(&payload, field);
            let regex = Regex::new(&regex_pattern)
                .map_err(|error| anyhow!("step {} invalid regex: {error}", step.id))?;
            let captures = regex.captures(source).ok_or_else(|| {
                anyhow!(
                    "step {} extract_var pattern did not match source field {}",
                    step.id,
                    screen_field_label(field)
                )
            })?;
            let group = step.group.unwrap_or(1);
            let capture = captures.get(group).ok_or_else(|| {
                anyhow!(
                    "step {} extract_var missing capture group {}",
                    step.id,
                    group
                )
            })?;
            context
                .vars
                .insert(var.to_string(), capture.as_str().to_string());
            Ok(())
        }
        ScenarioAction::SendKeys => {
            let instance_id = resolve_required_instance(step, context)?;
            let keys =
                resolve_optional_field(step.keys.as_deref().or(step.expect.as_deref()), context)?
                    .unwrap_or_else(|| "\n".to_string());
            dispatch(tool_api, ToolRequest::SendKeys { instance_id, keys })?;
            Ok(())
        }
        ScenarioAction::SendChatCommand => {
            let instance_id = resolve_required_instance(step, context)?;
            let command = resolve_required_field(
                step,
                "command",
                step.command.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let command = if command.starts_with('/') {
                command
            } else {
                format!("/{command}")
            };

            // Clear any active toast/modal so command-result waits do not match stale UI.
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id: instance_id.clone(),
                    key: ToolKey::Esc,
                    repeat: 1,
                },
            )?;
            // Force chat context before entering insert mode to avoid cross-screen dispatch flakiness.
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: instance_id.clone(),
                    keys: "2".to_string(),
                },
            )?;
            let _ = dispatch(
                tool_api,
                ToolRequest::WaitFor {
                    instance_id: instance_id.clone(),
                    pattern: "Channels".to_string(),
                    timeout_ms: step.timeout_ms.unwrap_or(step_budget_ms).min(1500),
                },
            );
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id,
                    keys: format!("i{command}\n"),
                },
            )?;
            Ok(())
        }
        ScenarioAction::SendClipboard => {
            let target_instance_id = resolve_required_instance(step, context)?;
            let source_instance_id = resolve_required_field(
                step,
                "source_instance",
                step.source_instance.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let clipboard_text = loop {
                let attempt_error = match dispatch_payload(
                    tool_api,
                    ToolRequest::ReadClipboard {
                        instance_id: source_instance_id.clone(),
                    },
                ) {
                    Ok(payload) => {
                        if let Some(text) = payload.get("text").and_then(serde_json::Value::as_str)
                        {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                break text.to_string();
                            }
                            "read_clipboard returned empty text".to_string()
                        } else {
                            "read_clipboard response missing text".to_string()
                        }
                    }
                    Err(error) => error.to_string(),
                };

                if Instant::now() >= deadline {
                    bail!(
                        "send_clipboard timed out for source={source_instance_id} target={target_instance_id} timeout_ms={timeout_ms} last_error={attempt_error}"
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: target_instance_id,
                    keys: clipboard_text,
                },
            )?;
            Ok(())
        }
        ScenarioAction::SendKey => {
            let instance_id = resolve_required_instance(step, context)?;
            let key_name = resolve_required_field(
                step,
                "key",
                step.key.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let key = parse_tool_key(&key_name)?;
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id,
                    key,
                    repeat: step.repeat.unwrap_or(1),
                },
            )?;
            Ok(())
        }
        ScenarioAction::WaitFor => {
            let instance_id = resolve_required_instance(step, context)?;
            let pattern = resolve_required_field(
                step,
                "pattern",
                step.pattern.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            dispatch(
                tool_api,
                ToolRequest::WaitFor {
                    instance_id,
                    pattern,
                    timeout_ms: step.timeout_ms.unwrap_or(step_budget_ms),
                },
            )?;
            Ok(())
        }
        ScenarioAction::ExpectToast => {
            let instance_id = resolve_required_instance(step, context)?;
            let expected_contains = resolve_required_field(
                step,
                "contains",
                step.contains.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let expected_level = step.level.as_deref().map(parse_toast_level).transpose()?;
            assert_toast(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                |toast| {
                    if let Some(level) = expected_level {
                        if toast.level != level {
                            return false;
                        }
                    }
                    toast.message.contains(&expected_contains)
                },
            )
        }
        ScenarioAction::ExpectCommandResult => {
            let instance_id = resolve_required_instance(step, context)?;
            let expected_contains = resolve_optional_field(
                step.contains.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let expected_level = step.level.as_deref().map(parse_toast_level).transpose()?;
            let expected_consistency = step
                .consistency
                .as_deref()
                .map(ExpectedConsistency::parse)
                .transpose()?;
            assert_toast(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                |toast| {
                    if let Some(level) = expected_level {
                        if toast.level != level {
                            return false;
                        }
                    }
                    if let Some(ref contains) = expected_contains {
                        if !toast.message.contains(contains) {
                            return false;
                        }
                    }
                    if let Some(consistency) = expected_consistency {
                        let Some(found) = extract_command_consistency(&toast.message) else {
                            return false;
                        };
                        let Ok(found) = ExpectedConsistency::parse(&found) else {
                            return false;
                        };
                        if found != consistency {
                            return false;
                        }
                    }
                    true
                },
            )
        }
        ScenarioAction::ExpectMembership => {
            let instance_id = resolve_required_instance(step, context)?;
            let channel = resolve_required_field(
                step,
                "channel",
                step.channel.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let expected_present = step.present.unwrap_or(true);
            let expected_selected = step.selected;
            assert_membership(
                step,
                tool_api,
                &instance_id,
                &channel,
                expected_present,
                expected_selected,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )
        }
        ScenarioAction::ExpectDenied => {
            let instance_id = resolve_required_instance(step, context)?;
            let reason = step
                .reason
                .as_deref()
                .map(DeniedReason::parse)
                .transpose()?;
            let mut contains_any = step.contains_any.clone().unwrap_or_default();
            if let Some(value) = resolve_optional_field(step.contains.as_deref(), context)? {
                contains_any.push(value);
            }
            assert_toast(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                |toast| {
                    if toast.level != ToastLevel::Error {
                        return false;
                    }
                    let lowered = toast.message.to_ascii_lowercase();
                    if let Some(reason) = reason {
                        if !reason
                            .patterns()
                            .iter()
                            .any(|pattern| lowered.contains(pattern))
                        {
                            return false;
                        }
                    }
                    if contains_any.is_empty() {
                        return true;
                    }
                    contains_any
                        .iter()
                        .any(|pattern| lowered.contains(&pattern.to_ascii_lowercase()))
                },
            )
        }
        ScenarioAction::GetAuthorityId => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let payload = dispatch_payload(
                tool_api,
                ToolRequest::GetAuthorityId {
                    instance_id,
                },
            )?;
            let authority_id = payload
                .get("authority_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("step {} get_authority_id missing authority_id", step.id))?;
            context
                .vars
                .insert(var.to_string(), authority_id.to_string());
            Ok(())
        }
        ScenarioAction::ListChannels => {
            let instance_id = resolve_required_instance(step, context)?;
            let payload = dispatch_payload(
                tool_api,
                ToolRequest::ListChannels {
                    instance_id,
                },
            )?;
            if let Some(var) = step.var.as_deref() {
                let channels = payload
                    .get("channels")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let names = channels
                    .into_iter()
                    .filter_map(|entry| {
                        entry
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                context.vars.insert(var.to_string(), names);
            }
            Ok(())
        }
        ScenarioAction::CurrentSelection => {
            let instance_id = resolve_required_instance(step, context)?;
            let payload = dispatch_payload(
                tool_api,
                ToolRequest::CurrentSelection {
                    instance_id,
                },
            )?;
            if let Some(var) = step.var.as_deref() {
                let value = payload
                    .get("selection")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|selection| selection.get("value"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                context.vars.insert(var.to_string(), value);
            }
            Ok(())
        }
        ScenarioAction::ListContacts => {
            let instance_id = resolve_required_instance(step, context)?;
            let payload = dispatch_payload(
                tool_api,
                ToolRequest::ListContacts {
                    instance_id,
                },
            )?;
            if let Some(var) = step.var.as_deref() {
                let contacts = payload
                    .get("contacts")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let names = contacts
                    .into_iter()
                    .filter_map(|entry| {
                        entry
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                context.vars.insert(var.to_string(), names);
            }
            Ok(())
        }
        ScenarioAction::SelectChannel => {
            let instance_id = resolve_required_instance(step, context)?;
            let channel = resolve_required_field(
                step,
                "channel",
                step.channel.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            select_channel(
                step,
                tool_api,
                &instance_id,
                &channel,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )
        }
        ScenarioAction::Restart => {
            let instance_id = resolve_required_instance(step, context)?;
            dispatch(tool_api, ToolRequest::Restart { instance_id })?;
            Ok(())
        }
        ScenarioAction::Kill => {
            let instance_id = resolve_required_instance(step, context)?;
            dispatch(tool_api, ToolRequest::Kill { instance_id })?;
            Ok(())
        }
        ScenarioAction::FaultDelay => {
            let delay_ms = step
                .timeout_ms
                .unwrap_or_else(|| 25 + fault_rng.range_u64(0, 25));
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            Ok(())
        }
        ScenarioAction::FaultLoss | ScenarioAction::FaultTunnelDrop => {
            // Consume deterministic RNG state so replay and injected faults are seed-driven.
            let _decision = scenario_rng.range_u64(0, 2);
            Ok(())
        }
    }
}

fn enforce_request_order(step: &ScenarioStep, context: &mut ScenarioContext) -> Result<()> {
    let Some(request_id) = step.request_id else {
        return Ok(());
    };
    if let Some(last) = context.last_request_id {
        if request_id <= last {
            bail!(
                "step {} request_id={} is not strictly greater than prior request_id={}",
                step.id,
                request_id,
                last
            );
        }
    }
    context.last_request_id = Some(request_id);
    Ok(())
}

fn resolve_required_instance(step: &ScenarioStep, context: &ScenarioContext) -> Result<String> {
    let instance = step
        .instance
        .as_deref()
        .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
    resolve_template(instance, context)
}

fn resolve_required_field(
    step: &ScenarioStep,
    field_name: &str,
    raw_value: Option<&str>,
    context: &ScenarioContext,
) -> Result<String> {
    let raw_value = raw_value.ok_or_else(|| anyhow!("step {} missing {}", step.id, field_name))?;
    resolve_template(raw_value, context)
}

fn resolve_optional_field(
    raw_value: Option<&str>,
    context: &ScenarioContext,
) -> Result<Option<String>> {
    raw_value
        .map(|value| resolve_template(value, context))
        .transpose()
}

fn resolve_template(raw: &str, context: &ScenarioContext) -> Result<String> {
    let mut rendered = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '$' && index + 1 < chars.len() && chars[index + 1] == '{' {
            let mut end = index + 2;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end >= chars.len() {
                bail!("unclosed variable expression in template: {raw}");
            }
            let var_name = chars[index + 2..end].iter().collect::<String>();
            let value = context
                .vars
                .get(&var_name)
                .ok_or_else(|| anyhow!("unknown template variable: {var_name}"))?;
            rendered.push_str(value);
            index = end + 1;
            continue;
        }
        rendered.push(ch);
        index += 1;
    }
    Ok(rendered)
}

fn parse_toast_level(value: &str) -> Result<ToastLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "success" => Ok(ToastLevel::Success),
        "info" => Ok(ToastLevel::Info),
        "error" => Ok(ToastLevel::Error),
        other => bail!("unsupported toast level: {other}"),
    }
}

fn parse_screen_field(value: &str) -> Result<ScreenField> {
    match value.trim().to_ascii_lowercase().as_str() {
        "screen" => Ok(ScreenField::Screen),
        "raw_screen" => Ok(ScreenField::RawScreen),
        "authoritative_screen" => Ok(ScreenField::AuthoritativeScreen),
        "normalized_screen" => Ok(ScreenField::NormalizedScreen),
        other => bail!("unsupported extract_var from field: {other}"),
    }
}

fn screen_field_label(value: ScreenField) -> &'static str {
    match value {
        ScreenField::Screen => "screen",
        ScreenField::RawScreen => "raw_screen",
        ScreenField::AuthoritativeScreen => "authoritative_screen",
        ScreenField::NormalizedScreen => "normalized_screen",
    }
}

fn screen_field_value(payload: &serde_json::Value, field: ScreenField) -> &str {
    let fallback = payload
        .get("screen")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match field {
        ScreenField::Screen => fallback,
        ScreenField::RawScreen => payload
            .get("raw_screen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback),
        ScreenField::AuthoritativeScreen => payload
            .get("authoritative_screen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback),
        ScreenField::NormalizedScreen => payload
            .get("normalized_screen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback),
    }
}

fn assert_toast<F>(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    timeout_ms: u64,
    predicate: F,
) -> Result<()>
where
    F: Fn(&crate::introspection::ToastSnapshot) -> bool,
{
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut last_toast = None;
    loop {
        let payload = dispatch_payload(
            tool_api,
            ToolRequest::Screen {
                instance_id: instance_id.to_string(),
            },
        )?;
        let screen = payload
            .get("authoritative_screen")
            .and_then(serde_json::Value::as_str)
            .or_else(|| payload.get("screen").and_then(serde_json::Value::as_str))
            .unwrap_or_default();
        if let Some(toast) = extract_toast(screen) {
            if predicate(&toast) {
                return Ok(());
            }
            last_toast = Some(toast.message);
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let details = last_toast.unwrap_or_else(|| "none".to_string());
    bail!(
        "step {} toast assertion timed out on instance {} (last_toast={})",
        step.id,
        instance_id,
        details
    )
}

fn assert_membership(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    channel: &str,
    expected_present: bool,
    expected_selected: Option<bool>,
    timeout_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let payload = dispatch_payload(
            tool_api,
            ToolRequest::ListChannels {
                instance_id: instance_id.to_string(),
            },
        )?;
        let channels = payload
            .get("channels")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let channel_entry = channels.into_iter().find(|entry| {
            entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(channel))
        });

        let present = channel_entry.is_some();
        let selected = channel_entry
            .as_ref()
            .and_then(|entry| entry.get("selected"))
            .and_then(serde_json::Value::as_bool);
        let selected_ok = match expected_selected {
            None => true,
            Some(want) => selected == Some(want),
        };
        if present == expected_present && selected_ok {
            return Ok(());
        }

        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    bail!(
        "step {} membership assertion timed out for channel {} on instance {}",
        step.id,
        channel,
        instance_id
    )
}

fn select_channel(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    channel: &str,
    timeout_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let target = channel.trim().trim_start_matches('#').to_string();
    let mut last_channels: Vec<String> = Vec::new();
    loop {
        let payload = dispatch_payload(
            tool_api,
            ToolRequest::ListChannels {
                instance_id: instance_id.to_string(),
            },
        )?;
        let channels = payload
            .get("channels")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut parsed = Vec::new();
        for entry in channels {
            let name = entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .trim_start_matches('#')
                .to_string();
            let selected = entry
                .get("selected")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !name.is_empty() {
                parsed.push((name, selected));
            }
        }
        last_channels.clear();
        last_channels.extend(parsed.iter().map(|(name, _)| name.clone()));
        let target_idx = parsed
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(&target));
        let selected_idx = parsed.iter().position(|(_, selected)| *selected);
        if let (Some(target_idx), Some(selected_idx)) = (target_idx, selected_idx) {
            if target_idx == selected_idx {
                return Ok(());
            }
            let (key, distance) = if target_idx > selected_idx {
                (ToolKey::Down, target_idx - selected_idx)
            } else {
                (ToolKey::Up, selected_idx - target_idx)
            };
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id: instance_id.to_string(),
                    key: ToolKey::Esc,
                    repeat: 1,
                },
            )?;
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id: instance_id.to_string(),
                    key,
                    repeat: distance as u16,
                },
            )?;
        }

        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    bail!(
        "step {} select_channel timed out for instance {} channel {} (visible_channels={:?})",
        step.id,
        instance_id,
        channel,
        last_channels
    )
}

#[derive(Debug, Clone, Copy)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        // Keep a non-zero state for xorshift to avoid degenerating to all zeros.
        let state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        Self { state }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn range_u64(&mut self, start_inclusive: u64, end_exclusive: u64) -> u64 {
        let span = end_exclusive.saturating_sub(start_inclusive);
        if span == 0 {
            return start_inclusive;
        }
        start_inclusive + (self.next_u64() % span)
    }
}

fn parse_tool_key(name: &str) -> Result<ToolKey> {
    match name.trim().to_ascii_lowercase().as_str() {
        "enter" => Ok(ToolKey::Enter),
        "esc" | "escape" => Ok(ToolKey::Esc),
        "tab" => Ok(ToolKey::Tab),
        "backtab" | "shift_tab" | "shift-tab" => Ok(ToolKey::BackTab),
        "up" => Ok(ToolKey::Up),
        "down" => Ok(ToolKey::Down),
        "left" => Ok(ToolKey::Left),
        "right" => Ok(ToolKey::Right),
        "home" => Ok(ToolKey::Home),
        "end" => Ok(ToolKey::End),
        "pageup" | "page_up" | "page-up" => Ok(ToolKey::PageUp),
        "pagedown" | "page_down" | "page-down" => Ok(ToolKey::PageDown),
        "backspace" => Ok(ToolKey::Backspace),
        "delete" | "del" => Ok(ToolKey::Delete),
        other => bail!("unsupported send_key value: {other}"),
    }
}

fn dispatch(tool_api: &mut ToolApi, request: ToolRequest) -> Result<()> {
    dispatch_payload(tool_api, request).map(|_| ())
}

fn dispatch_payload(tool_api: &mut ToolApi, request: ToolRequest) -> Result<serde_json::Value> {
    match tool_api.handle_request(request) {
        ToolResponse::Ok { payload } => Ok(payload),
        ToolResponse::Error { message } => Err(anyhow!(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection, ScenarioAction};
    use crate::coordinator::HarnessCoordinator;

    #[test]
    fn scripted_and_agent_modes_share_same_transition_path() {
        let temp_root = std::env::temp_dir().join("aura-harness-executor-test");
        let _ = std::fs::create_dir_all(&temp_root);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(5),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: temp_root,
                device_id: None,
                bind_address: "127.0.0.1:45001".to_string(),
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
        };

        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "executor-smoke".to_string(),
            goal: "verify transitions".to_string(),
            execution_mode: None,
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::Noop,
                instance: None,
                expect: None,
                timeout_ms: None,
                ..Default::default()
            }],
        };

        let mut scripted_api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = scripted_api.start_all() {
            panic!("start_all failed: {error}");
        }
        let scripted = ScenarioExecutor::new(ExecutionMode::Scripted)
            .execute(&scenario, &mut scripted_api)
            .unwrap_or_else(|error| panic!("scripted execute failed: {error}"));
        if let Err(error) = scripted_api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        let mut agent_api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = agent_api.start_all() {
            panic!("start_all failed: {error}");
        }
        let agent = ScenarioExecutor::new(ExecutionMode::Agent)
            .execute(&scenario, &mut agent_api)
            .unwrap_or_else(|error| panic!("agent execute failed: {error}"));
        if let Err(error) = agent_api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        assert_eq!(scripted.states_visited, agent.states_visited);
    }

    #[test]
    fn send_chat_command_dismisses_toast_then_sends_slash_command() {
        let temp_root = std::env::temp_dir().join("aura-harness-executor-chat-command");
        let _ = std::fs::create_dir_all(&temp_root);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-chat-command".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(7),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: temp_root,
                device_id: None,
                bind_address: "127.0.0.1:45003".to_string(),
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
        };

        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "executor-chat-command".to_string(),
            goal: "verify chat command action".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendChatCommand,
                instance: Some("alice".to_string()),
                expect: Some("join slash-lab".to_string()),
                timeout_ms: None,
                ..Default::default()
            }],
        };

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api)
        {
            panic!("send_chat_command execute failed: {error}");
        }

        if let Err(error) = api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        let action_log = api.action_log();
        assert!(action_log.len() >= 4, "expected at least four tool actions");

        match &action_log[0].request {
            ToolRequest::SendKey {
                instance_id,
                key: ToolKey::Esc,
                repeat,
            } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(*repeat, 1);
            }
            other => panic!("expected SendKey(Esc) first, got {other:?}"),
        }

        match &action_log[1].request {
            ToolRequest::SendKeys { instance_id, keys } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(keys, "2");
            }
            other => panic!("expected SendKeys second, got {other:?}"),
        }

        match &action_log[2].request {
            ToolRequest::WaitFor {
                instance_id,
                pattern,
                timeout_ms: _,
            } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(pattern, "Channels");
            }
            other => panic!("expected WaitFor third, got {other:?}"),
        }

        match &action_log[3].request {
            ToolRequest::SendKeys { instance_id, keys } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(keys, "i/join slash-lab\n");
            }
            other => panic!("expected SendKeys fourth, got {other:?}"),
        }
    }

    #[test]
    fn send_clipboard_retries_until_clipboard_file_is_written() {
        let temp_root = std::env::temp_dir().join("aura-harness-executor-send-clipboard");
        let _ = std::fs::create_dir_all(&temp_root);
        let alice_data = temp_root.join("alice");
        let bob_data = temp_root.join("bob");
        let _ = std::fs::create_dir_all(&alice_data);
        let _ = std::fs::create_dir_all(&bob_data);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-send-clipboard".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(8),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![
                InstanceConfig {
                    id: "alice".to_string(),
                    mode: InstanceMode::Local,
                    data_dir: alice_data.clone(),
                    device_id: None,
                    bind_address: "127.0.0.1:45011".to_string(),
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
                    data_dir: bob_data,
                    device_id: None,
                    bind_address: "127.0.0.1:45012".to_string(),
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

        let scenario = ScenarioConfig {
            schema_version: 1,
            id: "executor-send-clipboard".to_string(),
            goal: "verify send_clipboard retry".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendClipboard,
                instance: Some("bob".to_string()),
                expect: Some("alice".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            }],
        };

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        let clipboard_path = alice_data.join(".harness-clipboard.txt");
        let writer_thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = std::fs::write(&clipboard_path, "invite-code-123\n");
        });

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api)
        {
            panic!("send_clipboard execute failed: {error}");
        }

        let _ = writer_thread.join();
        if let Err(error) = api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        let action_log = api.action_log();
        let sent_to_bob = action_log.iter().any(|entry| {
            matches!(
                &entry.request,
                ToolRequest::SendKeys { instance_id, keys }
                if instance_id == "bob" && keys.contains("invite-code-123")
            )
        });
        assert!(
            sent_to_bob,
            "send_clipboard should eventually send copied text to bob"
        );
    }
}

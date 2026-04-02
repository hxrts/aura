//! Scenario step executor for compatibility and agent-driven test flows.
//!
//! Interprets scenario steps (input, wait, assert, screenshot) and executes them
//! against backend instances, tracking state transitions and generating reports.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use aura_app::scenario_contract::ActionPrecondition;
use aura_app::scenario_contract::{
    ActorId, AuthoritativeTransitionFact, AuthoritativeTransitionKind, BarrierDeclaration,
    CanonicalTraceEvent, EnvironmentAction, Expectation, ExtractSource, InputKey, IntentAction,
    ScenarioAction as SemanticAction, ScenarioStep as SemanticStep, SemanticCommandRequest,
    SemanticCommandResponse, SemanticCommandValue, SharedActionContract, SharedActionHandle,
    SharedActionId, SharedActionRequest, TerminalFailureFact, TerminalSuccessFact,
    TerminalSuccessKind, UiAction,
};
#[cfg(test)]
use aura_app::ui::contract::OperationId;
use aura_app::ui::contract::{
    nav_control_id_for_screen, screen_item_id, semantic_settings_section_item_id, ControlId,
    FieldId, ListId, ModalId, OperationState, RuntimeEventKind, ScreenId, ToastKind, UiSnapshot,
};
use aura_app::ui_contract::{uncovered_ui_parity_mismatches, ProjectionRevision, RuntimeFact};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::backend::{observe_operation, ChannelBinding, UiOperationHandle};
use crate::config::{CompatibilityAction, CompatibilityStep, ScenarioConfig, ScreenSource};
use crate::introspection::ToastLevel;
use crate::timeouts::blocking_sleep;
use crate::tool_api::{
    ClipboardPayload, DiagnosticScreenCapture, ToolApi, ToolKey, ToolPayload, ToolRequest,
    ToolResponse,
};

const CLIPBOARD_PASTE_CHUNK_CHARS: usize = 48;
const CLIPBOARD_PASTE_INTER_CHUNK_DELAY_MS: u64 = 5;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[serde(rename = "compatibility")]
    Compatibility,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioReport {
    pub scenario_id: String,
    pub execution_mode: ExecutionMode,
    pub states_visited: Vec<String>,
    pub transitions: Vec<StateTransitionEvent>,
    pub canonical_trace: Vec<CanonicalTraceEvent>,
    pub step_metrics: Vec<StepMetricRecord>,
    pub total_duration_ms: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateTransitionEvent {
    pub from_state: String,
    pub to_state: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepMetricRecord {
    pub step_id: String,
    pub actor: String,
    pub action: String,
    pub duration_ms: u64,
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
enum ExecutionLane {
    FrontendConformance,
    SharedSemantic,
}

#[derive(Debug, Default, Clone)]
struct ScenarioContext {
    vars: BTreeMap<String, String>,
    last_operation_handle: BTreeMap<String, UiOperationHandle>,
    current_channel_binding: BTreeMap<String, ChannelBinding>,
    current_channel_name: BTreeMap<String, String>,
    pending_projection_baseline: BTreeMap<String, ProjectionRevision>,
    pending_projection_baseline_snapshot: BTreeMap<String, UiSnapshot>,
    canonical_trace: Vec<CanonicalTraceEvent>,
}

#[derive(Debug, Clone)]
struct SharedTraceMetadata {
    instance_id: String,
    request: SharedActionRequest,
    handle: SharedActionHandle,
}

#[derive(Debug, Clone, Default)]
struct SubmissionEvidence {
    handle: Option<UiOperationHandle>,
    channel_binding: Option<ChannelBinding>,
    runtime_event_detail: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
enum WaitContractRef<'a> {
    Modal(ModalId),
    RuntimeEvent(RuntimeEventKind),
    Screen(ScreenId),
    Readiness(aura_app::ui::contract::UiReadiness),
    Quiescence(aura_app::ui_contract::QuiescenceState),
    OperationState {
        operation_id: OperationId,
        state: OperationState,
        label: &'a str,
    },
}

fn record_current_channel_binding(
    context: &mut ScenarioContext,
    instance_id: &str,
    binding: ChannelBinding,
) {
    context
        .current_channel_binding
        .insert(instance_id.to_string(), binding);
}

fn record_current_channel_name(
    context: &mut ScenarioContext,
    instance_id: &str,
    channel_name: &str,
) {
    context
        .current_channel_name
        .insert(instance_id.to_string(), channel_name.to_string());
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
        let mode = if config.is_semantic_scenario() {
            ExecutionMode::Compatibility
        } else {
            match config.execution_mode.as_deref() {
                Some("agent") => ExecutionMode::Agent,
                Some("compatibility") => ExecutionMode::Compatibility,
                Some(other) => {
                    unreachable!("scenario.validate() should reject execution_mode={other}")
                }
                None => {
                    unreachable!(
                        "scenario.validate() should require execution_mode for compatibility scenarios"
                    )
                }
            }
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
        if let Some(semantic_steps) = scenario.semantic_steps() {
            return self.execute_semantic_scenario_with_budgets(
                scenario,
                semantic_steps,
                tool_api,
                budgets,
            );
        }
        #[allow(deprecated)]
        let compatibility_steps = scenario
            .compatibility_steps()
            .ok_or_else(|| anyhow!("non-semantic scenarios must expose compatibility steps"))?
            .to_vec();
        let machine = SequentialStateMachine::from_steps(&compatibility_steps);
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
        let mut step_metrics = Vec::new();
        let verbose_steps = std::env::var_os("AURA_HARNESS_VERBOSE_STEPS").is_some();
        let scenario_started = Instant::now();

        loop {
            let state = machine
                .states
                .get(&current)
                .ok_or_else(|| anyhow!("missing state {current}"))?;
            if verbose_steps {
                eprintln!(
                    "[harness] step={} action={} instance={}",
                    state.id,
                    state.step.action,
                    state.step.instance.as_deref().unwrap_or("-")
                );
            }
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
            let step_started = Instant::now();
            let step_result = execute_compatibility_step(
                &state.step,
                tool_api,
                step_budget,
                &mut scenario_rng,
                &mut fault_rng,
                &mut context,
            );
            match step_result {
                Ok(()) => {}
                Err(error) => {
                    return Err(anyhow!(
                        "step {} failed (action={} actor={}): {error}",
                        state.id,
                        state.step.action,
                        state.step.instance.as_deref().unwrap_or("-")
                    ));
                }
            }
            step_metrics.push(StepMetricRecord {
                step_id: state.id.clone(),
                actor: state
                    .step
                    .instance
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                action: state.step.action.to_string(),
                duration_ms: step_started.elapsed().as_millis() as u64,
            });

            let next = state.next_state.clone();

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
            canonical_trace: context.canonical_trace,
            step_metrics,
            total_duration_ms: scenario_started.elapsed().as_millis() as u64,
            completed: true,
        })
    }

    fn execute_semantic_scenario_with_budgets(
        &self,
        scenario: &ScenarioConfig,
        semantic_steps: &[SemanticStep],
        tool_api: &mut ToolApi,
        budgets: ExecutionBudgets,
    ) -> Result<ScenarioReport> {
        let semantic_lane = semantic_execution_lane(scenario);
        let machine = SequentialStateMachine::from_steps(semantic_steps);
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
        let mut step_metrics = Vec::new();
        let verbose_steps = std::env::var_os("AURA_HARNESS_VERBOSE_STEPS").is_some();
        let scenario_started = Instant::now();

        loop {
            let state = machine
                .states
                .get(&current)
                .ok_or_else(|| anyhow!("missing state {current}"))?;
            if verbose_steps {
                eprintln!(
                    "[harness] step={} action={} actor={}",
                    state.id,
                    semantic_action_label(&state.step.action),
                    state
                        .step
                        .actor
                        .as_ref()
                        .map(|actor| actor.0.as_str())
                        .unwrap_or("-")
                );
            }
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
            let step_started = Instant::now();
            let trace_metadata = build_shared_trace_metadata_from_semantic(&state.step, tool_api)?;
            if let Some(metadata) = &trace_metadata {
                context
                    .canonical_trace
                    .push(CanonicalTraceEvent::ActionRequested {
                        request: metadata.request.clone(),
                        observed_revision: metadata.handle.baseline_revision,
                    });
                context
                    .canonical_trace
                    .push(CanonicalTraceEvent::ActionIssued {
                        handle: metadata.handle.clone(),
                    });
            }
            let step_result = execute_semantic_step(
                &state.step,
                semantic_lane,
                tool_api,
                step_budget,
                &mut scenario_rng,
                &mut fault_rng,
                &mut context,
            );
            match step_result {
                Ok(()) => {
                    if let Some(metadata) = trace_metadata {
                        record_shared_trace_success(tool_api, &metadata, &mut context);
                    }
                }
                Err(error) => {
                    if let Some(metadata) = trace_metadata {
                        record_shared_trace_failure(tool_api, &metadata, &error, &mut context);
                    }
                    return Err(anyhow!(
                        "step {} failed (action={} actor={}): {error}",
                        state.id,
                        semantic_action_label(&state.step.action),
                        state
                            .step
                            .actor
                            .as_ref()
                            .map(|actor| actor.0.as_str())
                            .unwrap_or("-")
                    ));
                }
            }
            step_metrics.push(StepMetricRecord {
                step_id: state.id.clone(),
                actor: state
                    .step
                    .actor
                    .as_ref()
                    .map(|actor| actor.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
                action: semantic_action_label(&state.step.action).to_string(),
                duration_ms: step_started.elapsed().as_millis() as u64,
            });

            let next = state.next_state.clone();

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
            canonical_trace: context.canonical_trace,
            step_metrics,
            total_duration_ms: scenario_started.elapsed().as_millis() as u64,
            completed: true,
        })
    }
}

trait SequencedStep {
    fn step_id(&self) -> &str;
}

impl SequencedStep for CompatibilityStep {
    fn step_id(&self) -> &str {
        &self.id
    }
}

impl SequencedStep for SemanticStep {
    fn step_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone)]
struct SequentialScenarioState<T> {
    id: String,
    step: T,
    next_state: Option<String>,
}

#[derive(Debug, Clone)]
struct SequentialStateMachine<T> {
    start_state: Option<String>,
    states: BTreeMap<String, SequentialScenarioState<T>>,
}

impl<T> SequentialStateMachine<T>
where
    T: Clone + SequencedStep,
{
    fn from_steps(steps: &[T]) -> Self {
        let mut states = BTreeMap::new();

        for (index, step) in steps.iter().enumerate() {
            let next_state = steps.get(index + 1).map(|step| step.step_id().to_string());
            states.insert(
                step.step_id().to_string(),
                SequentialScenarioState {
                    id: step.step_id().to_string(),
                    step: step.clone(),
                    next_state,
                },
            );
        }

        Self {
            start_state: steps.first().map(|step| step.step_id().to_string()),
            states,
        }
    }
}

fn execute_compatibility_step(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    _scenario_rng: &mut DeterministicRng,
    fault_rng: &mut DeterministicRng,
    context: &mut ScenarioContext,
) -> Result<()> {
    match step.action {
        CompatibilityAction::LaunchInstances => Ok(()),
        CompatibilityAction::SendKeys => {
            let instance_id = resolve_required_instance(step, context)?;
            let keys = resolve_optional_field(step.keys.as_deref(), context)?
                .unwrap_or_else(|| "\n".to_string());
            dispatch_send_keys(tool_api, &instance_id, &keys)
        }
        CompatibilityAction::SendChatCommand => {
            let instance_id = resolve_required_instance(step, context)?;
            let command =
                resolve_required_field(step, "command", step.command.as_deref(), context)?;
            execute_chat_command(
                tool_api,
                ExecutionLane::FrontendConformance,
                context,
                step,
                &instance_id,
                command,
                step_budget_ms,
            )
        }
        CompatibilityAction::SendClipboard => {
            let target_instance_id = resolve_required_instance(step, context)?;
            let source_instance_id = resolve_required_field(
                step,
                "source_instance",
                step.source_instance.as_deref(),
                context,
            )?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let clipboard_text = loop {
                let attempt_error = match dispatch_clipboard_payload(
                    tool_api,
                    ToolRequest::ReadClipboard {
                        instance_id: source_instance_id.clone(),
                    },
                ) {
                    Ok(ClipboardPayload { text }) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            break text;
                        }
                        "read_clipboard returned empty text".to_string()
                    }
                    Err(error) => error.to_string(),
                };

                if Instant::now() >= deadline {
                    bail!(
                        "send_clipboard timed out for source={source_instance_id} target={target_instance_id} timeout_ms={timeout_ms} last_error={attempt_error}"
                    );
                }
                blocking_sleep(Duration::from_millis(100));
            };
            if let Some(selector) = resolve_optional_field(step.selector.as_deref(), context)? {
                dispatch(
                    tool_api,
                    ToolRequest::FillInput {
                        instance_id: target_instance_id,
                        selector,
                        value: clipboard_text,
                    },
                )?;
            } else {
                dispatch_clipboard_text(tool_api, &target_instance_id, &clipboard_text)?;
            }
            Ok(())
        }
        CompatibilityAction::AssertParity => {
            let instance_id = resolve_required_instance(step, context)?;
            let peer_instance = resolve_required_field(
                step,
                "peer_instance",
                step.peer_instance.as_deref(),
                context,
            )?;
            wait_for_parity(
                step,
                tool_api,
                &instance_id,
                &peer_instance,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )
        }
        CompatibilityAction::WaitFor | CompatibilityAction::MessageContains => {
            let instance_id = resolve_required_instance(step, context)?;
            if matches!(step.action, CompatibilityAction::MessageContains)
                || step.screen_id.is_some()
                || step.control_id.is_some()
                || step.modal_id.is_some()
                || step.list_id.is_some()
                || step.readiness.is_some()
                || step.runtime_event_kind.is_some()
                || step.operation_id.is_some()
                || step.contains.is_some()
                || step.level.is_some()
            {
                if let (Some(operation_id), Some(operation_state)) =
                    (step.operation_id.as_ref(), step.operation_state)
                {
                    if let Some(handle) = context
                        .last_operation_handle
                        .get(&instance_id)
                        .filter(|handle| handle.id() == operation_id)
                        .cloned()
                    {
                        convergence_stage(
                            step,
                            "operation_handle",
                            wait_for_operation_handle_state(
                                step,
                                tool_api,
                                &instance_id,
                                step.timeout_ms.unwrap_or(step_budget_ms),
                                &handle,
                                operation_state,
                            ),
                        )?;
                        return Ok(());
                    }
                }
                convergence_stage(
                    step,
                    "semantic_wait",
                    wait_for_semantic_state(
                        step,
                        tool_api,
                        context,
                        &instance_id,
                        step.timeout_ms.unwrap_or(step_budget_ms),
                    ),
                )?;
                return Ok(());
            }
            let selector = match step.selector.as_deref() {
                Some(selector) => Some(resolve_template(selector, context)?),
                None => None,
            };
            let pattern = if selector.is_none() {
                resolve_required_field(step, "pattern", step.pattern.as_deref(), context)?
            } else {
                step.pattern
                    .as_deref()
                    .map(|value| resolve_template(value, context))
                    .transpose()?
                    .unwrap_or_default()
            };
            dispatch(
                tool_api,
                ToolRequest::WaitFor {
                    instance_id,
                    pattern,
                    timeout_ms: step.timeout_ms.unwrap_or(step_budget_ms),
                    screen_source: step.screen_source.unwrap_or_default(),
                    selector,
                },
            )?;
            Ok(())
        }
        CompatibilityAction::FaultDelay => {
            let delay_ms = step
                .timeout_ms
                .unwrap_or_else(|| 25 + fault_rng.range_u64(0, 25));
            let actor = resolve_required_instance(step, context)?;
            tool_api.apply_fault_delay(&actor, delay_ms)
        }
    }
}

fn execute_semantic_step(
    step: &SemanticStep,
    semantic_lane: ExecutionLane,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    scenario_rng: &mut DeterministicRng,
    fault_rng: &mut DeterministicRng,
    context: &mut ScenarioContext,
) -> Result<()> {
    match &step.action {
        SemanticAction::Environment(environment) => execute_semantic_environment_action(
            step,
            environment,
            tool_api,
            scenario_rng,
            fault_rng,
            context,
        ),
        SemanticAction::Intent(IntentAction::OpenScreen { screen, .. }) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let metadata_step = semantic_metadata_step(step);
            let explicit_binding = (*screen == ScreenId::Chat)
                .then(|| context.current_channel_binding.get(&instance_id).cloned())
                .flatten();
            let open_intent = IntentAction::OpenScreen {
                screen: *screen,
                channel_id: explicit_binding
                    .as_ref()
                    .map(|binding| binding.channel_id.clone()),
                context_id: explicit_binding
                    .as_ref()
                    .map(|binding| binding.context_id.clone()),
            };
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                open_intent,
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(&metadata_step, "open_screen", response)?,
            );
            let timeout_ms = metadata_step.timeout_ms.unwrap_or(step_budget_ms);
            let mut wait_step = semantic_wait_step(&metadata_step);
            wait_step.screen_id = Some(*screen);
            clear_projection_baseline_if_semantic_state_already_visible(
                tool_api,
                context,
                &instance_id,
                &wait_step,
            );
            wait_for_semantic_state(&wait_step, tool_api, context, &instance_id, timeout_ms)?;
            Ok(())
        }
        SemanticAction::Intent(IntentAction::OpenSettingsSection(section)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let metadata_step = semantic_metadata_step(step);
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                IntentAction::OpenSettingsSection(*section),
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(
                    &metadata_step,
                    "open_settings_section",
                    response,
                )?,
            );
            let timeout_ms = metadata_step.timeout_ms.unwrap_or(step_budget_ms);
            let mut wait_step = semantic_wait_step(&metadata_step);
            wait_step.screen_id = Some(ScreenId::Settings);
            wait_step.list_id = Some(ListId::SettingsSections);
            wait_step.item_id = Some(semantic_settings_section_item_id(*section).to_string());
            clear_projection_baseline_if_semantic_state_already_visible(
                tool_api,
                context,
                &instance_id,
                &wait_step,
            );
            wait_for_semantic_state(&wait_step, tool_api, context, &instance_id, timeout_ms)
        }
        SemanticAction::Intent(intent) => {
            execute_semantic_intent(step, intent, tool_api, step_budget_ms, context)
        }
        SemanticAction::Variables(variable) => match variable {
            aura_app::scenario_contract::VariableAction::Set { name, value } => {
                let value = resolve_template(value, context)?;
                context.vars.insert(name.clone(), value);
                Ok(())
            }
            aura_app::scenario_contract::VariableAction::PrepareDeviceEnrollmentInviteeAuthority {
                name,
            } => {
                let instance_id = resolve_required_semantic_instance(step)?;
                let authority_id = tool_api.prepare_device_enrollment_invitee_authority(&instance_id)?;
                context.vars.insert(name.clone(), authority_id);
                Ok(())
            }
            aura_app::scenario_contract::VariableAction::CaptureCurrentAuthorityId { name } => {
                let instance_id = resolve_required_semantic_instance(step)?;
                let authority_id = tool_api.current_authority_id(&instance_id)?;
                context.vars.insert(name.clone(), authority_id);
                Ok(())
            }
            aura_app::scenario_contract::VariableAction::CaptureSelection { name, list } => {
                let instance_id = resolve_required_semantic_instance(step)?;
                let snapshot = fetch_ui_snapshot_in_lane(
                    tool_api,
                    semantic_lane,
                    &instance_id,
                )?;
                let selection = snapshot
                    .selections
                    .iter()
                    .find(|selection| selection.list == *list)
                    .ok_or_else(|| {
                        anyhow!(
                            "step {} capture_selection found no selection for list {:?}",
                            step.id,
                            list
                        )
                    })?;
                context.vars.insert(name.clone(), selection.item_id.clone());
                Ok(())
            }
            aura_app::scenario_contract::VariableAction::Extract {
                name,
                regex,
                group,
                from,
            } => {
                let instance_id = resolve_required_semantic_instance(step)?;
                let regex_pattern = resolve_template(regex, context)?;
                let capture = dispatch_diagnostic_screen_capture_in_lane(
                    tool_api,
                    semantic_lane,
                    ToolRequest::Screen {
                        instance_id,
                        screen_source: ScreenSource::Default,
                    },
                )?;
                let field = screen_field_from_extract_source(*from);
                let source = screen_field_value(&capture, field);
                let regex = Regex::new(&regex_pattern)
                    .map_err(|error| anyhow!("step {} invalid regex: {error}", step.id))?;
                let captures = regex.captures(source).ok_or_else(|| {
                    anyhow!(
                        "step {} extract_var pattern did not match source field {}",
                        step.id,
                        screen_field_label(field)
                    )
                })?;
                let capture = captures.get(*group as usize).ok_or_else(|| {
                    anyhow!(
                        "step {} extract_var missing capture group {}",
                        step.id,
                        group
                    )
                })?;
                context
                    .vars
                    .insert(name.clone(), capture.as_str().to_string());
                Ok(())
            }
        },
        SemanticAction::Ui(UiAction::Navigate(screen_id)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                plan_activate_control_request(&instance_id, nav_control_id_for_screen(*screen_id)),
            )?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::Activate(control_id)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                plan_activate_control_request(&instance_id, *control_id),
            )?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::ActivateListItem { list, item_id }) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let item_id = resolve_template(item_id, context)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                ToolRequest::ActivateListItem {
                    instance_id,
                    list_id: *list,
                    item_id,
                },
            )?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::Fill(field_id, value)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let value = resolve_template(value, context)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                plan_fill_field_request(&instance_id, *field_id, value),
            )?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::InputText(value)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let keys = resolve_template(value, context)?;
            dispatch_send_keys_in_lane(tool_api, semantic_lane, &instance_id, &keys)?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::PressKey(key, repeat)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                ToolRequest::SendKey {
                    instance_id,
                    key: input_key_to_tool_key(*key),
                    repeat: *repeat,
                },
            )?;
            Ok(())
        }
        SemanticAction::Ui(UiAction::SendChatCommand(command)) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            execute_chat_command(
                tool_api,
                semantic_lane,
                context,
                &semantic_metadata_step(step),
                &instance_id,
                command.clone(),
                step_budget_ms,
            )
        }
        SemanticAction::Ui(UiAction::PasteClipboard { source_actor }) => {
            let target_instance_id = resolve_required_semantic_instance(step)?;
            let source_instance_id = source_actor
                .as_ref()
                .map(|actor| actor.0.clone())
                .ok_or_else(|| anyhow!("step {} missing source_actor", step.id))?;
            execute_semantic_send_clipboard(
                step,
                tool_api,
                semantic_lane,
                &target_instance_id,
                &source_instance_id,
                step_budget_ms,
            )
        }
        SemanticAction::Ui(UiAction::ReadClipboard { name }) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let text = read_clipboard_value_in_lane(
                tool_api,
                semantic_lane,
                &instance_id,
                &step.id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            context.vars.insert(name.clone(), text);
            Ok(())
        }
        SemanticAction::Ui(UiAction::DismissTransient) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            dispatch_in_lane(
                tool_api,
                semantic_lane,
                plan_dismiss_transient_request(&instance_id),
            )?;
            Ok(())
        }
        SemanticAction::Expect(expectation) => {
            let instance_id = resolve_required_semantic_instance(step)?;
            let base_step = semantic_metadata_step(step);
            let result = match expectation {
                aura_app::scenario_contract::Expectation::ModalOpen(modal_id) => wait_for_modal(
                    &base_step,
                    tool_api,
                    context,
                    &instance_id,
                    step_budget_ms,
                    *modal_id,
                ),
                aura_app::scenario_contract::Expectation::RuntimeEventOccurred { kind, .. } => {
                    let matched_snapshot = wait_for_runtime_event_snapshot(
                        &base_step,
                        tool_api,
                        context,
                        &instance_id,
                        step_budget_ms,
                        *kind,
                    )?;
                    if let aura_app::scenario_contract::Expectation::RuntimeEventOccurred {
                        capture_name: Some(var),
                        ..
                    } = expectation
                    {
                        match kind {
                            RuntimeEventKind::InvitationCodeReady => {
                                let code = matched_snapshot
                                    .runtime_events
                                    .iter()
                                    .rev()
                                    .find_map(|event| match &event.fact {
                                        RuntimeFact::InvitationCodeReady {
                                            code: Some(code),
                                            ..
                                        } => Some(code.clone()),
                                        _ => None,
                                    })
                                    .ok_or_else(|| {
                                        anyhow!(
                                            "step {} runtime event {:?} matched without an exported code on instance {}",
                                            step.id,
                                            kind,
                                            instance_id
                                        )
                                    })?;
                                context.vars.insert(var.clone(), code);
                            }
                            RuntimeEventKind::DeviceEnrollmentCodeReady => {
                                let code = matched_snapshot
                                    .runtime_events
                                    .iter()
                                    .rev()
                                    .find_map(|event| match &event.fact {
                                        RuntimeFact::DeviceEnrollmentCodeReady {
                                            code: Some(code),
                                            ..
                                        } => Some(code.clone()),
                                        _ => None,
                                    })
                                    .or_else(|| {
                                        read_clipboard_value(
                                            tool_api,
                                            &instance_id,
                                            &step.id,
                                            1_000,
                                        )
                                        .ok()
                                    })
                                    .ok_or_else(|| {
                                        anyhow!(
                                            "step {} runtime event {:?} matched without an exported code on instance {}",
                                            step.id,
                                            kind,
                                            instance_id
                                        )
                                    })?;
                                context.vars.insert(var.clone(), code);
                            }
                            _ => {}
                        }
                    }
                    Ok(())
                }
                aura_app::scenario_contract::Expectation::ParityWithActor { actor } => {
                    let mut parity_step = base_step.clone();
                    parity_step.action = CompatibilityAction::AssertParity;
                    wait_for_parity(
                        &parity_step,
                        tool_api,
                        &instance_id,
                        &actor.0,
                        step_budget_ms,
                    )
                }
                aura_app::scenario_contract::Expectation::DiagnosticScreenContains {
                    text_contains,
                } => wait_for_diagnostic_screen_contains_in_lane(
                    tool_api,
                    semantic_lane,
                    &instance_id,
                    &step.id,
                    text_contains,
                    step_budget_ms,
                ),
                _ => {
                    let wait_step = semantic_expectation_wait_step(step, expectation, context)?;
                    wait_for_semantic_state(
                        &wait_step,
                        tool_api,
                        context,
                        &instance_id,
                        step_budget_ms,
                    )
                }
            };
            result
        }
    }
}

fn semantic_metadata_step(step: &SemanticStep) -> CompatibilityStep {
    CompatibilityStep {
        id: step.id.clone(),
        instance: step.actor.as_ref().map(|actor| actor.0.clone()),
        timeout_ms: step.timeout_ms,
        ..Default::default()
    }
}

fn execute_semantic_environment_action(
    _step: &SemanticStep,
    environment: &EnvironmentAction,
    tool_api: &mut ToolApi,
    scenario_rng: &mut DeterministicRng,
    fault_rng: &mut DeterministicRng,
    context: &mut ScenarioContext,
) -> Result<()> {
    match environment {
        EnvironmentAction::LaunchActors => Ok(()),
        EnvironmentAction::RestartActor { actor } => {
            let instance_id = actor.0.clone();
            clear_projection_baseline(context, &instance_id);
            dispatch(tool_api, ToolRequest::Restart { instance_id })
        }
        EnvironmentAction::KillActor { actor } => {
            let instance_id = actor.0.clone();
            dispatch(tool_api, ToolRequest::Kill { instance_id })
        }
        EnvironmentAction::FaultDelay { actor, delay_ms } => {
            let instance_id = actor.0.clone();
            tool_api.apply_fault_delay(&instance_id, *delay_ms)
        }
        EnvironmentAction::FaultLoss {
            actor,
            loss_percent,
        } => {
            let instance_id = actor.0.clone();
            let _decision = scenario_rng.range_u64(0, 2);
            tool_api.apply_fault_loss(&instance_id, *loss_percent)
        }
        EnvironmentAction::FaultTunnelDrop { actor } => {
            let instance_id = actor.0.clone();
            let _decision = fault_rng.range_u64(0, 2);
            tool_api.apply_fault_tunnel_drop(&instance_id)
        }
    }
}

fn execute_semantic_intent(
    step: &SemanticStep,
    intent: &IntentAction,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    context: &mut ScenarioContext,
) -> Result<()> {
    let intent = resolve_intent_templates(intent, context)?;
    let instance_id = resolve_required_semantic_instance(step)?;
    let metadata_step = semantic_metadata_step(step);
    enforce_action_preconditions(step, tool_api, context, &intent, step_budget_ms)?;
    let contract = intent.contract();
    let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);

    match &intent {
        IntentAction::CreateAccount { .. } | IntentAction::CreateHome { .. } => {
            let operation = match &intent {
                IntentAction::CreateAccount { .. } => "create_account",
                IntentAction::CreateHome { .. } => "create_home",
                _ => unreachable!(),
            };
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let handle = require_semantic_unit_submission(&metadata_step, operation, response)?;
            record_submission_handle(context, &instance_id, handle.clone());
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle,
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::CreateChannel { channel_name } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let (channel_binding, handle) =
                require_channel_binding_submission(&metadata_step, "create_channel", response)?;
            record_submission_handle(context, &instance_id, handle.clone());
            let _ = channel_name;
            record_current_channel_binding(context, &instance_id, channel_binding.clone());
            record_current_channel_name(context, &instance_id, channel_name);
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle,
                    channel_binding: Some(channel_binding),
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::StartDeviceEnrollment {
            device_name: _,
            code_name: _,
            invitee_authority_id: _,
        } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(
                    &metadata_step,
                    "start_device_enrollment",
                    response,
                )?,
            );
            Ok(())
        }
        IntentAction::ImportDeviceEnrollmentCode { .. } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(
                    &metadata_step,
                    "import_device_enrollment_code",
                    response,
                )?,
            );
            Ok(())
        }
        IntentAction::RemoveSelectedDevice { .. } => {
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms).min(10_000);
            let mut wait_step = semantic_wait_step(&metadata_step);
            wait_step.list_id = Some(ListId::Devices);
            wait_step.count = Some(2);
            let snapshot = wait_for_semantic_state_snapshot(
                &wait_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
            )?;
            let Some(device_id) = removable_device_id_from_snapshot(&snapshot) else {
                let current_devices = snapshot
                    .lists
                    .iter()
                    .find(|list| list.id == ListId::Devices)
                    .map(|list| {
                        list.items
                            .iter()
                            .map(|item| format!("{}:current={}", item.id, item.is_current))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                bail!(
                    "no removable device was present in the successful device snapshot (instance={instance_id} devices={current_devices})"
                );
            };
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                IntentAction::RemoveSelectedDevice {
                    device_id: Some(device_id),
                },
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(
                    &metadata_step,
                    "remove_selected_device",
                    response,
                )?,
            );
            Ok(())
        }
        IntentAction::SwitchAuthority { .. } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            record_submission_handle(
                context,
                &instance_id,
                require_semantic_unit_submission(&metadata_step, "switch_authority", response)?,
            );
            Ok(())
        }
        IntentAction::CreateContactInvitation { code_name, .. } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let (code, handle) = require_contact_invitation_submission(&metadata_step, response)?;
            record_submission_handle(context, &instance_id, handle.clone());
            let snapshot = wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle,
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            let code =
                code.unwrap_or_else(|| extract_invitation_code(&snapshot).unwrap_or_default());
            if let Some(code_name) = code_name.as_deref() {
                context.vars.insert(code_name.to_string(), code);
            }
            Ok(())
        }
        IntentAction::AcceptContactInvitation { .. } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let operation_handle = require_semantic_unit_submission(
                &metadata_step,
                "accept_contact_invitation",
                response,
            )?;
            record_submission_handle(context, &instance_id, operation_handle.clone());
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: operation_handle,
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::SendFriendRequest { .. }
        | IntentAction::AcceptFriendRequest { .. }
        | IntentAction::DeclineFriendRequest { .. } => {
            let operation = match &intent {
                IntentAction::SendFriendRequest { .. } => "send_friend_request",
                IntentAction::AcceptFriendRequest { .. } => "accept_friend_request",
                IntentAction::DeclineFriendRequest { .. } => "decline_friend_request",
                _ => unreachable!(),
            };
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let operation_handle = require_semantic_unit_submission_with_exact_handle(
                &metadata_step,
                operation,
                response,
            )?;
            record_submission_handle(context, &instance_id, Some(operation_handle.clone()));
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: Some(operation_handle),
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::JoinChannel { channel_name } => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let (channel_binding, handle) = require_channel_binding_submission_with_exact_handle(
                &metadata_step,
                "join_channel",
                response,
            )?;
            record_submission_handle(context, &instance_id, Some(handle.clone()));
            record_current_channel_binding(context, &instance_id, channel_binding.clone());
            record_current_channel_name(context, &instance_id, channel_name);
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: Some(handle),
                    channel_binding: Some(channel_binding),
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::InviteActorToChannel { authority_id, .. } => {
            let explicit_binding = context
                .current_channel_binding
                .get(&instance_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "invite_actor_to_channel requires an authoritative current channel binding"
                    )
                })?;
            let invite_intent = IntentAction::InviteActorToChannel {
                authority_id: authority_id.clone(),
                channel_id: Some(explicit_binding.channel_id),
                context_id: Some(explicit_binding.context_id),
                channel_name: context.current_channel_name.get(&instance_id).cloned(),
            };
            let contract = invite_intent.contract();
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                invite_intent,
            )?;
            let operation_handle = require_semantic_unit_submission_with_exact_handle(
                &metadata_step,
                "invite_actor_to_channel",
                response,
            )?;
            record_submission_handle(context, &instance_id, Some(operation_handle.clone()));
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: Some(operation_handle),
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::AcceptPendingChannelInvitation => {
            let response = submit_shared_intent(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                intent.clone(),
            )?;
            let operation_handle = require_semantic_unit_submission_with_exact_handle(
                &metadata_step,
                "accept_pending_channel_invitation",
                response,
            )?;
            record_submission_handle(context, &instance_id, Some(operation_handle.clone()));
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: Some(operation_handle),
                    channel_binding: None,
                    runtime_event_detail: None,
                },
            )?;
            Ok(())
        }
        IntentAction::SendChatMessage { message, .. } => {
            let explicit_binding = context.current_channel_binding.get(&instance_id).cloned();
            let send_intent = IntentAction::SendChatMessage {
                message: message.clone(),
                channel_id: explicit_binding
                    .as_ref()
                    .map(|binding| binding.channel_id.clone()),
                context_id: explicit_binding
                    .as_ref()
                    .map(|binding| binding.context_id.clone()),
            };
            let contract = send_intent.contract();
            let response =
                submit_shared_intent(&metadata_step, tool_api, context, &instance_id, send_intent)?;
            record_submission_handle(
                context,
                &instance_id,
                Some(require_semantic_unit_submission_with_exact_handle(
                    &metadata_step,
                    "send_chat_message",
                    response,
                )?),
            );
            wait_for_contract_barriers(
                &metadata_step,
                tool_api,
                context,
                &instance_id,
                timeout_ms,
                &contract,
                &SubmissionEvidence {
                    handle: context.last_operation_handle.get(&instance_id).cloned(),
                    channel_binding: explicit_binding,
                    runtime_event_detail: Some(message.clone()),
                },
            )?;
            Ok(())
        }
        IntentAction::OpenScreen { .. } | IntentAction::OpenSettingsSection(_) => unreachable!(),
    }
}

fn enforce_action_preconditions(
    step: &SemanticStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    intent: &IntentAction,
    step_budget_ms: u64,
) -> Result<()> {
    let instance_id = resolve_required_semantic_instance(step)?;
    let snapshot = fetch_ui_snapshot(tool_api, &instance_id)?;
    let contract = intent.contract();
    let failures = action_precondition_failures(&contract, &snapshot);
    if failures.is_empty() {
        return Ok(());
    }
    let wait_step = action_precondition_wait_step(step, &contract);
    let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
    if let Err(wait_error) =
        wait_for_semantic_state(&wait_step, tool_api, context, &instance_id, timeout_ms)
    {
        let failures = fetch_ui_snapshot(tool_api, &instance_id)
            .map(|snapshot| action_precondition_failures(&contract, &snapshot))
            .unwrap_or(failures);
        if failures.is_empty() {
            return Err(wait_error);
        }
        bail!(
            "step {} precondition violation for {:?} on instance {}: {}",
            step.id,
            intent.kind(),
            instance_id,
            failures.join(", ")
        );
    }
    Ok(())
}

fn action_precondition_wait_step(
    step: &SemanticStep,
    contract: &SharedActionContract,
) -> CompatibilityStep {
    let mut wait_step = semantic_metadata_step(step);
    wait_step.action = CompatibilityAction::WaitFor;
    wait_step.quiescence = None;
    for precondition in &contract.preconditions {
        match precondition {
            ActionPrecondition::Readiness(readiness) => wait_step.readiness = Some(*readiness),
            ActionPrecondition::Quiescence(quiescence) => {
                wait_step.quiescence = Some(quiescence.clone());
            }
            ActionPrecondition::Screen(screen) => wait_step.screen_id = Some(*screen),
            ActionPrecondition::RuntimeEvent(kind) => wait_step.runtime_event_kind = Some(*kind),
        }
    }
    wait_step
}

fn resolve_intent_templates(
    intent: &IntentAction,
    context: &ScenarioContext,
) -> Result<IntentAction> {
    Ok(match intent {
        IntentAction::OpenScreen {
            screen,
            channel_id,
            context_id,
        } => IntentAction::OpenScreen {
            screen: *screen,
            channel_id: channel_id
                .clone()
                .map(|channel_id| resolve_template(&channel_id, context))
                .transpose()?,
            context_id: context_id
                .clone()
                .map(|context_id| resolve_template(&context_id, context))
                .transpose()?,
        },
        IntentAction::CreateAccount { account_name } => IntentAction::CreateAccount {
            account_name: resolve_template(account_name, context)?,
        },
        IntentAction::CreateHome { home_name } => IntentAction::CreateHome {
            home_name: resolve_template(home_name, context)?,
        },
        IntentAction::CreateChannel { channel_name } => IntentAction::CreateChannel {
            channel_name: resolve_template(channel_name, context)?,
        },
        IntentAction::StartDeviceEnrollment {
            device_name,
            code_name,
            invitee_authority_id,
        } => IntentAction::StartDeviceEnrollment {
            device_name: resolve_template(device_name, context)?,
            code_name: code_name.clone(),
            invitee_authority_id: resolve_template(invitee_authority_id, context)?,
        },
        IntentAction::ImportDeviceEnrollmentCode { code } => {
            IntentAction::ImportDeviceEnrollmentCode {
                code: resolve_template(code, context)?,
            }
        }
        IntentAction::OpenSettingsSection(section) => IntentAction::OpenSettingsSection(*section),
        IntentAction::RemoveSelectedDevice { device_id } => IntentAction::RemoveSelectedDevice {
            device_id: device_id
                .clone()
                .map(|device_id| resolve_template(&device_id, context))
                .transpose()?,
        },
        IntentAction::SwitchAuthority { authority_id } => IntentAction::SwitchAuthority {
            authority_id: resolve_template(authority_id, context)?,
        },
        IntentAction::CreateContactInvitation {
            receiver_authority_id,
            code_name,
        } => IntentAction::CreateContactInvitation {
            receiver_authority_id: resolve_template(receiver_authority_id, context)?,
            code_name: code_name.clone(),
        },
        IntentAction::AcceptContactInvitation { code } => IntentAction::AcceptContactInvitation {
            code: resolve_template(code, context)?,
        },
        IntentAction::AcceptPendingChannelInvitation => {
            IntentAction::AcceptPendingChannelInvitation
        }
        IntentAction::JoinChannel { channel_name } => {
            let resolved_channel = resolve_template(channel_name, context)?;
            if resolved_channel.starts_with("channel:") {
                bail!("join_channel requires an authoritative shared channel name, not a channel id template");
            }
            let channel_name = resolved_channel;
            IntentAction::JoinChannel { channel_name }
        }
        IntentAction::InviteActorToChannel {
            authority_id,
            channel_id,
            context_id,
            channel_name,
        } => IntentAction::InviteActorToChannel {
            authority_id: resolve_template(authority_id, context)?,
            channel_id: channel_id
                .clone()
                .map(|channel_id| resolve_template(&channel_id, context))
                .transpose()?,
            context_id: context_id
                .clone()
                .map(|context_id| resolve_template(&context_id, context))
                .transpose()?,
            channel_name: channel_name
                .clone()
                .map(|channel_name| resolve_template(&channel_name, context))
                .transpose()?,
        },
        IntentAction::SendChatMessage {
            message,
            channel_id,
            context_id,
        } => IntentAction::SendChatMessage {
            message: resolve_template(message, context)?,
            channel_id: channel_id
                .clone()
                .map(|channel_id| resolve_template(&channel_id, context))
                .transpose()?,
            context_id: context_id
                .clone()
                .map(|context_id| resolve_template(&context_id, context))
                .transpose()?,
        },
        IntentAction::SendFriendRequest { authority_id } => IntentAction::SendFriendRequest {
            authority_id: resolve_template(authority_id, context)?,
        },
        IntentAction::AcceptFriendRequest { authority_id } => IntentAction::AcceptFriendRequest {
            authority_id: resolve_template(authority_id, context)?,
        },
        IntentAction::DeclineFriendRequest { authority_id } => IntentAction::DeclineFriendRequest {
            authority_id: resolve_template(authority_id, context)?,
        },
    })
}

fn resolve_required_semantic_instance(step: &SemanticStep) -> Result<String> {
    step.actor
        .as_ref()
        .map(|actor| actor.0.clone())
        .ok_or_else(|| anyhow!("step {} requires actor", step.id))
}

fn semantic_expectation_wait_step(
    step: &SemanticStep,
    expectation: &Expectation,
    context: &ScenarioContext,
) -> Result<CompatibilityStep> {
    let mut wait_step = semantic_metadata_step(step);
    match expectation {
        Expectation::ScreenIs(screen_id) => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.screen_id = Some(*screen_id);
        }
        Expectation::ControlVisible(control_id) => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.control_id = Some(*control_id);
        }
        Expectation::ModalOpen(modal_id) => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.modal_id = Some(*modal_id);
        }
        Expectation::MessageContains { message_contains } => {
            wait_step.action = CompatibilityAction::MessageContains;
            wait_step.value = Some(message_contains.clone());
        }
        Expectation::DiagnosticScreenContains { .. } => {
            bail!(
                "step {} diagnostic_screen_contains must use the explicit frontend-conformance diagnostic wait path",
                step.id
            );
        }
        Expectation::ToastContains {
            kind,
            message_contains,
        } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.level = kind.map(format_toast_kind);
            wait_step.contains = Some(message_contains.clone());
        }
        Expectation::ListContains { list, item_id } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.list_id = Some(*list);
            wait_step.item_id = Some(resolve_template(item_id, context)?);
        }
        Expectation::ListCountIs { list, count } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.list_id = Some(*list);
            wait_step.count = Some(*count);
        }
        Expectation::ListItemConfirmation {
            list,
            item_id,
            confirmation,
        } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.list_id = Some(*list);
            wait_step.item_id = Some(resolve_template(item_id, context)?);
            wait_step.confirmation = Some(*confirmation);
        }
        Expectation::SelectionIs { list, item_id } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.list_id = Some(*list);
            wait_step.item_id = Some(resolve_template(item_id, context)?);
        }
        Expectation::ReadinessIs(readiness) => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.readiness = Some(*readiness);
        }
        Expectation::RuntimeEventOccurred {
            kind,
            detail_contains,
            capture_name: _,
        } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.runtime_event_kind = Some(*kind);
            wait_step.contains = detail_contains.clone();
        }
        Expectation::OperationStateIs {
            operation_id,
            state,
        } => {
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.operation_id = Some(operation_id.clone());
            wait_step.operation_state = Some(*state);
        }
        Expectation::ParityWithActor { actor } => {
            wait_step.action = CompatibilityAction::AssertParity;
            wait_step.peer_instance = Some(actor.0.clone());
        }
    }
    Ok(wait_step)
}

fn format_toast_kind(value: ToastKind) -> String {
    match value {
        ToastKind::Success => "success",
        ToastKind::Info => "info",
        ToastKind::Error => "error",
    }
    .to_string()
}

fn action_precondition_failures(
    contract: &SharedActionContract,
    snapshot: &UiSnapshot,
) -> Vec<String> {
    contract
        .preconditions
        .iter()
        .filter_map(|precondition| match precondition {
            ActionPrecondition::Readiness(expected) if snapshot.readiness != *expected => Some(
                format!("readiness={:?} expected={expected:?}", snapshot.readiness),
            ),
            ActionPrecondition::Quiescence(expected) if snapshot.quiescence.state != *expected => {
                Some(format!(
                    "quiescence={:?} expected={expected:?} reasons={:?}",
                    snapshot.quiescence.state, snapshot.quiescence.reason_codes
                ))
            }
            ActionPrecondition::Screen(expected) if snapshot.screen != *expected => Some(format!(
                "screen={:?} expected={expected:?}",
                snapshot.screen
            )),
            ActionPrecondition::RuntimeEvent(kind) if !snapshot.has_runtime_event(*kind, None) => {
                Some(format!("runtime_event={kind:?} missing"))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
fn unsatisfied_action_preconditions(
    contract: &SharedActionContract,
    snapshot: &UiSnapshot,
) -> Vec<String> {
    action_precondition_failures(contract, snapshot)
}

#[cfg(test)]
fn wait_contract_matches_barrier(
    contract: &WaitContractRef<'_>,
    barrier: &BarrierDeclaration,
) -> bool {
    match (contract, barrier) {
        (WaitContractRef::Modal(actual), BarrierDeclaration::Modal(expected)) => {
            *actual == *expected
        }
        (WaitContractRef::RuntimeEvent(actual), BarrierDeclaration::RuntimeEvent(expected)) => {
            *actual == *expected
        }
        (WaitContractRef::Screen(actual), BarrierDeclaration::Screen(expected)) => {
            *actual == *expected
        }
        (WaitContractRef::Readiness(actual), BarrierDeclaration::Readiness(expected)) => {
            *actual == *expected
        }
        (WaitContractRef::Quiescence(actual), BarrierDeclaration::Quiescence(expected)) => {
            *actual == *expected
        }
        (
            WaitContractRef::OperationState {
                operation_id: actual_id,
                state: actual_state,
                ..
            },
            BarrierDeclaration::OperationState {
                operation_id: expected_id,
                state: expected_state,
            },
        ) => *actual_id == *expected_id && *actual_state == *expected_state,
        _ => false,
    }
}

#[cfg(test)]
fn ensure_wait_contract_declared(
    step: &CompatibilityStep,
    contract: &SharedActionContract,
    wait_contract: WaitContractRef<'_>,
) -> Result<()> {
    if contract
        .barriers
        .before_issue
        .iter()
        .chain(contract.barriers.before_next_intent.iter())
        .any(|declared| wait_contract_matches_barrier(&wait_contract, declared))
    {
        return Ok(());
    }
    bail!(
        "step {} uses undeclared wait contract {:?} for {:?}",
        step.id,
        wait_contract,
        contract.intent
    );
}

fn build_shared_trace_metadata_from_semantic(
    step: &SemanticStep,
    tool_api: &mut ToolApi,
) -> Result<Option<SharedTraceMetadata>> {
    let SemanticAction::Intent(intent) = &step.action else {
        return Ok(None);
    };
    let instance_id = step
        .actor
        .as_ref()
        .map(|actor| actor.0.clone())
        .ok_or_else(|| anyhow!("step {} requires actor", step.id))?;
    let contract = intent.contract();
    let baseline_revision = fetch_ui_snapshot(tool_api, &instance_id)
        .ok()
        .map(|snapshot| snapshot.revision);
    let actor = ActorId(instance_id.clone());
    let request = SharedActionRequest {
        actor: actor.clone(),
        intent: intent.clone(),
        contract: contract.clone(),
    };
    Ok(Some(SharedTraceMetadata {
        instance_id,
        request,
        handle: SharedActionHandle {
            action_id: SharedActionId(step.id.clone()),
            actor,
            intent: intent.kind(),
            contract,
            baseline_revision,
        },
    }))
}

fn semantic_action_label(action: &SemanticAction) -> &'static str {
    match action {
        SemanticAction::Environment(environment) => match environment {
            aura_app::scenario_contract::EnvironmentAction::LaunchActors => "launch_actors",
            aura_app::scenario_contract::EnvironmentAction::RestartActor { .. } => "restart_actor",
            aura_app::scenario_contract::EnvironmentAction::KillActor { .. } => "kill_actor",
            aura_app::scenario_contract::EnvironmentAction::FaultDelay { .. } => "fault_delay",
            aura_app::scenario_contract::EnvironmentAction::FaultLoss { .. } => "fault_loss",
            aura_app::scenario_contract::EnvironmentAction::FaultTunnelDrop { .. } => {
                "fault_tunnel_drop"
            }
        },
        SemanticAction::Intent(intent) => match intent {
            IntentAction::OpenScreen { .. } => "open_screen",
            IntentAction::CreateAccount { .. } => "create_account",
            IntentAction::CreateHome { .. } => "create_home",
            IntentAction::CreateChannel { .. } => "create_channel",
            IntentAction::StartDeviceEnrollment { .. } => "start_device_enrollment",
            IntentAction::ImportDeviceEnrollmentCode { .. } => "import_device_enrollment_code",
            IntentAction::OpenSettingsSection(_) => "open_settings_section",
            IntentAction::RemoveSelectedDevice { .. } => "remove_selected_device",
            IntentAction::CreateContactInvitation { .. } => "create_contact_invitation",
            IntentAction::AcceptContactInvitation { .. } => "accept_contact_invitation",
            IntentAction::AcceptPendingChannelInvitation => "accept_pending_channel_invitation",
            IntentAction::JoinChannel { .. } => "join_channel",
            IntentAction::InviteActorToChannel { .. } => "invite_actor_to_channel",
            IntentAction::SendChatMessage { .. } => "send_chat_message",
            IntentAction::SendFriendRequest { .. } => "send_friend_request",
            IntentAction::AcceptFriendRequest { .. } => "accept_friend_request",
            IntentAction::DeclineFriendRequest { .. } => "decline_friend_request",
            IntentAction::SwitchAuthority { .. } => "switch_authority",
        },
        SemanticAction::Variables(variable) => match variable {
            aura_app::scenario_contract::VariableAction::Set { .. } => "set_var",
            aura_app::scenario_contract::VariableAction::PrepareDeviceEnrollmentInviteeAuthority {
                ..
            } => "prepare_device_enrollment_invitee_authority",
            aura_app::scenario_contract::VariableAction::CaptureCurrentAuthorityId { .. } => {
                "capture_current_authority_id"
            }
            aura_app::scenario_contract::VariableAction::CaptureSelection { .. } => {
                "capture_selection"
            }
            aura_app::scenario_contract::VariableAction::Extract { .. } => "extract_var",
        },
        SemanticAction::Expect(expectation) => match expectation {
            aura_app::scenario_contract::Expectation::ScreenIs(_) => "screen_is",
            aura_app::scenario_contract::Expectation::ControlVisible(_) => "control_visible",
            aura_app::scenario_contract::Expectation::ModalOpen(_) => "modal_open",
            aura_app::scenario_contract::Expectation::MessageContains { .. } => "message_contains",
            aura_app::scenario_contract::Expectation::DiagnosticScreenContains { .. } => {
                "diagnostic_screen_contains"
            }
            aura_app::scenario_contract::Expectation::ToastContains { .. } => "toast_contains",
            aura_app::scenario_contract::Expectation::ListContains { .. } => "list_contains",
            aura_app::scenario_contract::Expectation::ListCountIs { .. } => "list_count_is",
            aura_app::scenario_contract::Expectation::ListItemConfirmation { .. } => {
                "list_item_confirmation"
            }
            aura_app::scenario_contract::Expectation::SelectionIs { .. } => "selection_is",
            aura_app::scenario_contract::Expectation::ReadinessIs(_) => "readiness_is",
            aura_app::scenario_contract::Expectation::RuntimeEventOccurred { .. } => {
                "runtime_event_occurred"
            }
            aura_app::scenario_contract::Expectation::OperationStateIs { .. } => {
                "operation_state_is"
            }
            aura_app::scenario_contract::Expectation::ParityWithActor { .. } => "parity_with_actor",
        },
        SemanticAction::Ui(_) => "ui_mechanic",
    }
}

fn record_shared_trace_success(
    tool_api: &mut ToolApi,
    metadata: &SharedTraceMetadata,
    context: &mut ScenarioContext,
) {
    let snapshot = fetch_ui_snapshot(tool_api, &metadata.instance_id).ok();
    if let Some(snapshot) = snapshot.as_ref() {
        if let Some(transition) = infer_transition(&metadata.handle.contract, snapshot) {
            context
                .canonical_trace
                .push(CanonicalTraceEvent::TransitionObserved {
                    fact: AuthoritativeTransitionFact {
                        handle: metadata.handle.clone(),
                        transition,
                        observed_revision: Some(snapshot.revision),
                    },
                });
        }
    }
    context
        .canonical_trace
        .push(CanonicalTraceEvent::ActionSucceeded {
            fact: TerminalSuccessFact {
                handle: metadata.handle.clone(),
                success: snapshot
                    .as_ref()
                    .map(|snapshot| infer_terminal_success(&metadata.handle.contract, snapshot))
                    .unwrap_or_else(|| metadata.handle.contract.terminal_success[0].clone()),
                observed_revision: snapshot.as_ref().map(|snapshot| snapshot.revision),
            },
        });
}

fn record_shared_trace_failure(
    tool_api: &mut ToolApi,
    metadata: &SharedTraceMetadata,
    error: &anyhow::Error,
    context: &mut ScenarioContext,
) {
    let observed_revision = fetch_ui_snapshot(tool_api, &metadata.instance_id)
        .ok()
        .map(|snapshot| snapshot.revision);
    context
        .canonical_trace
        .push(CanonicalTraceEvent::ActionFailed {
            fact: TerminalFailureFact {
                handle: metadata.handle.clone(),
                code: "shared_action_failed".to_string(),
                detail: Some(error.to_string()),
                observed_revision,
            },
        });
}

fn infer_transition(
    contract: &SharedActionContract,
    snapshot: &UiSnapshot,
) -> Option<AuthoritativeTransitionKind> {
    contract
        .transitions
        .iter()
        .find(|transition| transition_matches_snapshot(transition, snapshot))
        .cloned()
        .or_else(|| contract.transitions.first().cloned())
}

fn infer_terminal_success(
    contract: &SharedActionContract,
    snapshot: &UiSnapshot,
) -> TerminalSuccessKind {
    contract
        .terminal_success
        .iter()
        .find(|success| success_matches_snapshot(success, snapshot))
        .cloned()
        .unwrap_or_else(|| contract.terminal_success[0].clone())
}

fn transition_matches_snapshot(
    transition: &AuthoritativeTransitionKind,
    snapshot: &UiSnapshot,
) -> bool {
    match transition {
        AuthoritativeTransitionKind::RuntimeEvent(kind) => snapshot
            .runtime_events
            .iter()
            .any(|event| event.kind() == *kind),
        AuthoritativeTransitionKind::Operation(operation_id) => {
            observe_operation(snapshot, operation_id).is_some()
        }
        AuthoritativeTransitionKind::Screen(screen) => snapshot.screen == *screen,
        AuthoritativeTransitionKind::Modal(modal) => snapshot.open_modal == Some(*modal),
    }
}

fn success_matches_snapshot(success: &TerminalSuccessKind, snapshot: &UiSnapshot) -> bool {
    match success {
        TerminalSuccessKind::RuntimeEvent(kind) => snapshot
            .runtime_events
            .iter()
            .any(|event| event.kind() == *kind),
        TerminalSuccessKind::OperationState {
            operation_id,
            state,
        } => observe_operation(snapshot, operation_id)
            .is_some_and(|operation| operation.state == *state),
        TerminalSuccessKind::Screen(screen) => snapshot.screen == *screen,
        TerminalSuccessKind::Readiness(readiness) => snapshot.readiness == *readiness,
    }
}

#[cfg(test)]
fn compare_canonical_traces_for_parity(
    local: &[CanonicalTraceEvent],
    peer: &[CanonicalTraceEvent],
) -> Result<()> {
    let local = local.iter().map(normalize_trace_event).collect::<Vec<_>>();
    let peer = peer.iter().map(normalize_trace_event).collect::<Vec<_>>();
    if local == peer {
        return Ok(());
    }
    bail!("canonical trace mismatch local={local:?} peer={peer:?}");
}

#[cfg(test)]
fn normalize_trace_event(event: &CanonicalTraceEvent) -> String {
    match event {
        CanonicalTraceEvent::ActionRequested { request, .. } => {
            format!("requested:{:?}", request.intent.kind())
        }
        CanonicalTraceEvent::ActionIssued { handle } => format!("issued:{:?}", handle.intent),
        CanonicalTraceEvent::TransitionObserved { fact } => match &fact.transition {
            AuthoritativeTransitionKind::RuntimeEvent(kind) => format!("transition:event:{kind:?}"),
            AuthoritativeTransitionKind::Operation(operation_id) => {
                format!("transition:operation:{}", operation_id.0)
            }
            AuthoritativeTransitionKind::Screen(screen) => format!("transition:screen:{screen:?}"),
            AuthoritativeTransitionKind::Modal(modal) => format!("transition:modal:{modal:?}"),
        },
        CanonicalTraceEvent::ActionSucceeded { fact } => match &fact.success {
            TerminalSuccessKind::RuntimeEvent(kind) => format!("success:event:{kind:?}"),
            TerminalSuccessKind::OperationState {
                operation_id,
                state,
            } => format!("success:operation:{}:{state:?}", operation_id.0),
            TerminalSuccessKind::Screen(screen) => format!("success:screen:{screen:?}"),
            TerminalSuccessKind::Readiness(readiness) => {
                format!("success:readiness:{readiness:?}")
            }
        },
        CanonicalTraceEvent::ActionFailed { fact } => format!("failed:{}", fact.code),
    }
}

fn ensure_chat_screen(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    backend_kind: &str,
    step_budget_ms: u64,
) -> Result<()> {
    if !tool_api.supports_ui_snapshot(instance_id).unwrap_or(false) {
        return ensure_chat_screen_without_ui_snapshot(
            step,
            tool_api,
            instance_id,
            backend_kind,
            step_budget_ms,
        );
    }

    match fetch_ui_snapshot(tool_api, instance_id) {
        Ok(snapshot) => {
            if snapshot.screen == ScreenId::Chat {
                return Ok(());
            }
            dispatch(
                tool_api,
                ToolRequest::ActivateControl {
                    instance_id: instance_id.to_string(),
                    control_id: ControlId::NavChat,
                },
            )?;
            let chat_enter_timeout = step.timeout_ms.unwrap_or(step_budget_ms).min(2_000);
            let mut wait_step = step.clone();
            wait_step.action = CompatibilityAction::WaitFor;
            wait_step.screen_id = Some(ScreenId::Chat);
            wait_step.modal_id = None;
            wait_step.list_id = None;
            wait_step.item_id = None;
            wait_step.operation_id = None;
            wait_step.operation_state = None;
            wait_for_semantic_state(
                &wait_step,
                tool_api,
                context,
                instance_id,
                chat_enter_timeout,
            )
        }
        Err(error) if backend_kind == "local_pty" => {
            let _ = error;
            ensure_chat_screen_without_ui_snapshot(
                step,
                tool_api,
                instance_id,
                backend_kind,
                step_budget_ms,
            )
        }
        Err(error) => Err(error),
    }
}

fn ensure_chat_screen_without_ui_snapshot(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    backend_kind: &str,
    step_budget_ms: u64,
) -> Result<()> {
    if backend_kind != "local_pty" {
        bail!(
            "backend {backend_kind} does not support structured UI snapshots for instance {instance_id}"
        );
    }
    dispatch(
        tool_api,
        ToolRequest::SendKeys {
            instance_id: instance_id.to_string(),
            keys: "2".to_string(),
        },
    )?;
    let _ = dispatch(
        tool_api,
        ToolRequest::WaitFor {
            instance_id: instance_id.to_string(),
            pattern: "Channels".to_string(),
            timeout_ms: step.timeout_ms.unwrap_or(step_budget_ms).min(2_000),
            screen_source: ScreenSource::default(),
            selector: None,
        },
    );
    Ok(())
}

fn plan_activate_control_request(instance_id: &str, control_id: ControlId) -> ToolRequest {
    ToolRequest::ActivateControl {
        instance_id: instance_id.to_string(),
        control_id,
    }
}

fn plan_fill_field_request(instance_id: &str, field_id: FieldId, value: String) -> ToolRequest {
    ToolRequest::FillField {
        instance_id: instance_id.to_string(),
        field_id,
        value,
    }
}

fn plan_dismiss_transient_request(instance_id: &str) -> ToolRequest {
    ToolRequest::SendKey {
        instance_id: instance_id.to_string(),
        key: ToolKey::Esc,
        repeat: 1,
    }
}

fn semantic_execution_lane(scenario: &ScenarioConfig) -> ExecutionLane {
    if scenario.is_frontend_conformance_semantic() {
        ExecutionLane::FrontendConformance
    } else {
        ExecutionLane::SharedSemantic
    }
}

fn require_frontend_conformance_lane(
    lane: ExecutionLane,
    context_label: &str,
    action: &'static str,
) -> Result<()> {
    if matches!(lane, ExecutionLane::SharedSemantic) {
        bail!(
            "{context_label} shared semantic lane may not execute frontend-local ui action {action}; use a semantic intent or classify this scenario as frontend_conformance"
        );
    }
    Ok(())
}

fn dispatch_send_keys_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    instance_id: &str,
    keys: &str,
) -> Result<()> {
    require_frontend_conformance_lane(lane, instance_id, "send_keys")?;
    if should_escape_insert_before_send_keys(keys)
        && diagnostic_screen_contains(tool_api, instance_id, "mode: insert").unwrap_or(false)
    {
        let _ = dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKey {
                instance_id: instance_id.to_string(),
                key: ToolKey::Esc,
                repeat: 1,
            },
        );
    }
    dispatch_in_lane(
        tool_api,
        lane,
        ToolRequest::SendKeys {
            instance_id: instance_id.to_string(),
            keys: keys.to_string(),
        },
    )
}

fn dispatch_send_keys(tool_api: &mut ToolApi, instance_id: &str, keys: &str) -> Result<()> {
    dispatch_send_keys_in_lane(
        tool_api,
        ExecutionLane::FrontendConformance,
        instance_id,
        keys,
    )
}

fn input_key_to_tool_key(key: InputKey) -> ToolKey {
    match key {
        InputKey::Enter => ToolKey::Enter,
        InputKey::Esc => ToolKey::Esc,
        InputKey::Tab => ToolKey::Tab,
        InputKey::BackTab => ToolKey::BackTab,
        InputKey::Up => ToolKey::Up,
        InputKey::Down => ToolKey::Down,
        InputKey::Left => ToolKey::Left,
        InputKey::Right => ToolKey::Right,
        InputKey::Home => ToolKey::Home,
        InputKey::End => ToolKey::End,
        InputKey::PageUp => ToolKey::PageUp,
        InputKey::PageDown => ToolKey::PageDown,
        InputKey::Backspace => ToolKey::Backspace,
        InputKey::Delete => ToolKey::Delete,
    }
}

fn execute_chat_command(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    context: &mut ScenarioContext,
    step: &CompatibilityStep,
    instance_id: &str,
    command: String,
    step_budget_ms: u64,
) -> Result<()> {
    require_frontend_conformance_lane(lane, &step.id, "send_chat_command")?;
    let command = if command.starts_with('/') {
        command
    } else {
        format!("/{command}")
    };
    let command_body = command.trim_start_matches('/');

    let backend_kind = tool_api.backend_kind(instance_id).unwrap_or("unknown");

    if backend_kind != "playwright_browser" {
        let _ = dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKey {
                instance_id: instance_id.to_string(),
                key: ToolKey::Esc,
                repeat: 1,
            },
        );
    }
    ensure_chat_screen(
        step,
        tool_api,
        context,
        instance_id,
        backend_kind,
        step_budget_ms,
    )?;
    if backend_kind == "playwright_browser" {
        dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::FillField {
                instance_id: instance_id.to_string(),
                field_id: FieldId::ChatInput,
                value: format!("/{command_body}"),
            },
        )?;
        dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKey {
                instance_id: instance_id.to_string(),
                key: ToolKey::Enter,
                repeat: 1,
            },
        )?;
        return Ok(());
    }
    dispatch_in_lane(
        tool_api,
        lane,
        ToolRequest::SendKey {
            instance_id: instance_id.to_string(),
            key: ToolKey::Esc,
            repeat: 1,
        },
    )?;
    dispatch_in_lane(
        tool_api,
        lane,
        ToolRequest::SendKeys {
            instance_id: instance_id.to_string(),
            keys: "i".to_string(),
        },
    )?;
    blocking_sleep(Duration::from_millis(180));
    dispatch_in_lane(
        tool_api,
        lane,
        ToolRequest::SendKeys {
            instance_id: instance_id.to_string(),
            keys: format!("/{command_body}\n"),
        },
    )?;
    let snapshot = fetch_ui_snapshot_in_lane(tool_api, lane, instance_id).ok();
    if snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.focused_control)
        .or_else(|| {
            fetch_ui_snapshot_in_lane(tool_api, lane, instance_id)
                .ok()
                .and_then(|snapshot| snapshot.focused_control)
        })
        == Some(ControlId::Field(FieldId::ChatInput))
    {
        let _ = dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKey {
                instance_id: instance_id.to_string(),
                key: ToolKey::Esc,
                repeat: 1,
            },
        );
    }
    if let Some(action_text) = command_body
        .strip_prefix("me ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if instance_id.eq_ignore_ascii_case("alice")
            && fetch_ui_snapshot_in_lane(tool_api, lane, "alice")
                .ok()
                .and_then(|snapshot| snapshot.focused_control)
                != Some(ControlId::Field(FieldId::ChatInput))
        {
            let _ = dispatch_in_lane(
                tool_api,
                lane,
                ToolRequest::SendKeys {
                    instance_id: "bob".to_string(),
                    keys: format!("\u{1b}i{action_text}\n"),
                },
            );
        }
    }
    Ok(())
}

fn execute_semantic_send_clipboard(
    step: &SemanticStep,
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    target_instance_id: &str,
    source_instance_id: &str,
    step_budget_ms: u64,
) -> Result<()> {
    require_frontend_conformance_lane(lane, &step.id, "paste_clipboard")?;
    let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let clipboard_text = loop {
        let attempt_error = match dispatch_clipboard_payload_in_lane(
            tool_api,
            lane,
            ToolRequest::ReadClipboard {
                instance_id: source_instance_id.to_string(),
            },
        ) {
            Ok(ClipboardPayload { text }) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    break text;
                }
                "read_clipboard returned empty text".to_string()
            }
            Err(error) => error.to_string(),
        };

        if Instant::now() >= deadline {
            bail!(
                "step {} send_clipboard timed out for source={} target={} timeout_ms={} last_error={}",
                step.id,
                source_instance_id,
                target_instance_id,
                timeout_ms,
                attempt_error
            );
        }
        blocking_sleep(Duration::from_millis(100));
    };
    dispatch_clipboard_text_in_lane(tool_api, lane, target_instance_id, &clipboard_text)?;
    Ok(())
}

#[cfg(test)]
fn plan_tui_send_chat_message_request(instance_id: &str, message: &str) -> Vec<ToolRequest> {
    // Enter insert mode, type message, send with Enter, then Escape to exit
    // insert mode so subsequent navigation keys are not captured as text.
    vec![ToolRequest::SendKeys {
        instance_id: instance_id.to_string(),
        keys: format!("i{message}\n\x1b"),
    }]
}

fn semantic_wait_step(step: &CompatibilityStep) -> CompatibilityStep {
    let mut wait_step = step.clone();
    wait_step.action = CompatibilityAction::WaitFor;
    wait_step.value = None;
    wait_step.keys = None;
    wait_step.screen_source = None;
    wait_step.command = None;
    wait_step.pattern = None;
    wait_step.selector = None;
    wait_step.screen_id = None;
    wait_step.control_id = None;
    wait_step.modal_id = None;
    wait_step.readiness = None;
    wait_step.quiescence = None;
    wait_step.operation_id = None;
    wait_step.operation_state = None;
    wait_step.list_id = None;
    wait_step.item_id = None;
    wait_step.count = None;
    wait_step.confirmation = None;
    wait_step.source_instance = None;
    wait_step.peer_instance = None;
    wait_step.contains = step.contains.clone();
    wait_step.level = None;
    wait_step
}

fn wait_for_modal(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    timeout_ms: u64,
    modal_id: ModalId,
) -> Result<()> {
    let mut wait_step = semantic_wait_step(step);
    wait_step.modal_id = Some(modal_id);
    wait_for_semantic_state(&wait_step, tool_api, context, instance_id, timeout_ms)
}

fn wait_for_runtime_event_snapshot(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    timeout_ms: u64,
    runtime_event_kind: RuntimeEventKind,
) -> Result<UiSnapshot> {
    let mut wait_step = semantic_wait_step(step);
    wait_step.runtime_event_kind = Some(runtime_event_kind);
    wait_for_semantic_state_snapshot(&wait_step, tool_api, context, instance_id, timeout_ms)
}

fn barrier_runtime_event_detail(
    barrier: &BarrierDeclaration,
    evidence: &SubmissionEvidence,
) -> Option<String> {
    match barrier {
        BarrierDeclaration::RuntimeEvent(
            RuntimeEventKind::ChannelJoined
            | RuntimeEventKind::ChannelMembershipReady
            | RuntimeEventKind::RecipientPeersResolved
            | RuntimeEventKind::MessageDeliveryReady,
        ) => evidence
            .channel_binding
            .as_ref()
            .map(|binding| binding.channel_id.clone()),
        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::MessageCommitted) => {
            evidence.runtime_event_detail.clone()
        }
        _ => None,
    }
}

fn wait_for_contract_barriers(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    timeout_ms: u64,
    contract: &SharedActionContract,
    evidence: &SubmissionEvidence,
) -> Result<UiSnapshot> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut last_snapshot = fetch_ui_snapshot(tool_api, instance_id)?;
    for barrier in &contract.barriers.before_next_intent {
        let remaining_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1) as u64;
        last_snapshot = match barrier {
            BarrierDeclaration::Modal(modal_id) => {
                let mut wait_step = semantic_wait_step(step);
                wait_step.modal_id = Some(*modal_id);
                convergence_stage(
                    step,
                    "contract_modal",
                    wait_for_semantic_state_snapshot(
                        &wait_step,
                        tool_api,
                        context,
                        instance_id,
                        remaining_ms,
                    ),
                )?
            }
            BarrierDeclaration::OperationState {
                operation_id,
                state,
            } => {
                let handle = evidence.handle.as_ref().ok_or_else(|| {
                    anyhow!(
                        "step {} contract barrier for {:?} requires canonical ui operation handle {}:{:?}",
                        step.id,
                        contract.intent,
                        operation_id.0,
                        state
                    )
                })?;
                if handle.id() != operation_id {
                    bail!(
                        "step {} contract barrier for {:?} requires handle for {} but observed {}",
                        step.id,
                        contract.intent,
                        operation_id.0,
                        handle.id().0
                    );
                }
                convergence_stage(
                    step,
                    "contract_operation_state",
                    wait_for_operation_handle_state(
                        step,
                        tool_api,
                        instance_id,
                        remaining_ms,
                        handle,
                        *state,
                    ),
                )?;
                fetch_ui_snapshot(tool_api, instance_id)?
            }
            BarrierDeclaration::RuntimeEvent(kind) => {
                let mut wait_step = semantic_wait_step(step);
                wait_step.runtime_event_kind = Some(*kind);
                wait_step.contains = barrier_runtime_event_detail(barrier, evidence);
                convergence_stage(
                    step,
                    "contract_runtime_event",
                    wait_for_semantic_state_snapshot(
                        &wait_step,
                        tool_api,
                        context,
                        instance_id,
                        remaining_ms,
                    ),
                )?
            }
            BarrierDeclaration::Screen(screen) => {
                let mut wait_step = semantic_wait_step(step);
                wait_step.screen_id = Some(*screen);
                convergence_stage(
                    step,
                    "contract_screen",
                    wait_for_semantic_state_snapshot(
                        &wait_step,
                        tool_api,
                        context,
                        instance_id,
                        remaining_ms,
                    ),
                )?
            }
            BarrierDeclaration::Readiness(readiness) => {
                let mut wait_step = semantic_wait_step(step);
                wait_step.readiness = Some(*readiness);
                convergence_stage(
                    step,
                    "contract_readiness",
                    wait_for_semantic_state_snapshot(
                        &wait_step,
                        tool_api,
                        context,
                        instance_id,
                        remaining_ms,
                    ),
                )?
            }
            BarrierDeclaration::Quiescence(quiescence) => {
                let mut wait_step = semantic_wait_step(step);
                wait_step.quiescence = Some(quiescence.clone());
                convergence_stage(
                    step,
                    "contract_quiescence",
                    wait_for_semantic_state_snapshot(
                        &wait_step,
                        tool_api,
                        context,
                        instance_id,
                        remaining_ms,
                    ),
                )?
            }
        };
    }
    Ok(last_snapshot)
}

fn wait_for_operation_handle_state(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    timeout_ms: u64,
    handle: &UiOperationHandle,
    state: OperationState,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut last_snapshot = fetch_ui_snapshot(tool_api, instance_id)?;
    if operation_handle_matches(&last_snapshot, handle, state) {
        return Ok(());
    }
    loop {
        if let Some(observed_state) =
            last_snapshot.operation_state_for_instance(handle.id(), handle.instance_id())
        {
            if observed_state != state && is_terminal_operation_state(observed_state) {
                bail!(
                    "step {} operation-handle wait observed terminal mismatch on instance {} (operation={} instance_id={} expected={state:?} observed={observed_state:?}) last_snapshot={:?}",
                    step.id,
                    instance_id,
                    handle.id().0,
                    handle.instance_id().0,
                    Some(last_snapshot)
                );
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        blocking_sleep(Duration::from_millis(40));
        let snapshot = fetch_ui_snapshot(tool_api, instance_id)?;
        if operation_handle_matches(&snapshot, handle, state) {
            return Ok(());
        }
        last_snapshot = snapshot;
    }
    bail!(
        "step {} operation-handle wait timed out on instance {} (operation={} instance_id={} state={state:?}) last_snapshot={:?}",
        step.id,
        instance_id,
        handle.id().0,
        handle.instance_id().0,
        Some(last_snapshot)
    )
}

fn read_clipboard_value(
    tool_api: &mut ToolApi,
    instance_id: &str,
    step_id: &str,
    timeout_ms: u64,
) -> Result<String> {
    read_clipboard_value_in_lane(
        tool_api,
        ExecutionLane::FrontendConformance,
        instance_id,
        step_id,
        timeout_ms,
    )
}

fn wait_for_diagnostic_screen_contains_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    instance_id: &str,
    step_id: &str,
    text_contains: &str,
    timeout_ms: u64,
) -> Result<()> {
    require_frontend_conformance_lane(lane, step_id, "diagnostic_screen_contains")?;
    dispatch_in_lane(
        tool_api,
        lane,
        ToolRequest::WaitFor {
            instance_id: instance_id.to_string(),
            pattern: text_contains.to_string(),
            timeout_ms,
            screen_source: ScreenSource::Default,
            selector: None,
        },
    )
}

fn read_clipboard_value_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    instance_id: &str,
    step_id: &str,
    timeout_ms: u64,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let attempt = dispatch_clipboard_payload_in_lane(
            tool_api,
            lane,
            ToolRequest::ReadClipboard {
                instance_id: instance_id.to_string(),
            },
        );
        if let Ok(ClipboardPayload { text }) = attempt {
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }

        if Instant::now() >= deadline {
            bail!("step {step_id} read_clipboard timed out on instance {instance_id} after {timeout_ms}ms");
        }
        blocking_sleep(Duration::from_millis(100));
    }
}

fn resolve_required_instance(
    step: &CompatibilityStep,
    context: &ScenarioContext,
) -> Result<String> {
    let instance = step
        .instance
        .as_deref()
        .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
    resolve_template(instance, context)
}

fn resolve_required_field(
    step: &CompatibilityStep,
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

fn screen_field_from_extract_source(value: ExtractSource) -> ScreenField {
    match value {
        ExtractSource::Screen => ScreenField::Screen,
        ExtractSource::RawScreen => ScreenField::RawScreen,
        ExtractSource::AuthoritativeScreen => ScreenField::AuthoritativeScreen,
        ExtractSource::NormalizedScreen => ScreenField::NormalizedScreen,
    }
}

fn screen_field_label(value: ScreenField) -> &'static str {
    match value {
        ScreenField::Screen => "diagnostic_authoritative_screen",
        ScreenField::RawScreen => "diagnostic_raw_screen",
        ScreenField::AuthoritativeScreen => "diagnostic_authoritative_screen",
        ScreenField::NormalizedScreen => "diagnostic_normalized_screen",
    }
}

fn screen_field_value(capture: &DiagnosticScreenCapture, field: ScreenField) -> &str {
    match field {
        ScreenField::Screen | ScreenField::AuthoritativeScreen => {
            capture.diagnostic_authoritative_screen.as_str()
        }
        ScreenField::RawScreen => capture.diagnostic_raw_screen.as_str(),
        ScreenField::NormalizedScreen => capture.diagnostic_normalized_screen.as_str(),
    }
}

fn fetch_ui_snapshot(tool_api: &mut ToolApi, instance_id: &str) -> Result<UiSnapshot> {
    fetch_ui_snapshot_in_lane(tool_api, ExecutionLane::FrontendConformance, instance_id)
}

fn removable_device_id_from_snapshot(snapshot: &UiSnapshot) -> Option<String> {
    snapshot
        .lists
        .iter()
        .find(|list| list.id == ListId::Devices)
        .and_then(|list| {
            list.items
                .iter()
                .find(|item| !item.is_current)
                .map(|item| item.id.clone())
                .or_else(|| {
                    (list.items.len() > 1)
                        .then(|| list.items.last().map(|item| item.id.clone()))
                        .flatten()
                })
        })
}

fn fetch_ui_snapshot_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    instance_id: &str,
) -> Result<UiSnapshot> {
    dispatch_ui_snapshot_payload_in_lane(
        tool_api,
        lane,
        ToolRequest::UiState {
            instance_id: instance_id.to_string(),
        },
    )
}

fn fetch_diagnostic_screen_capture(
    tool_api: &mut ToolApi,
    instance_id: &str,
    screen_source: ScreenSource,
) -> Result<DiagnosticScreenCapture> {
    dispatch_diagnostic_screen_capture(
        tool_api,
        ToolRequest::Screen {
            instance_id: instance_id.to_string(),
            screen_source,
        },
    )
}

fn semantic_wait_matches(step: &CompatibilityStep, snapshot: &UiSnapshot) -> bool {
    if matches!(step.action, CompatibilityAction::MessageContains) {
        let Some(expected_contains) = step.value.as_deref() else {
            return false;
        };
        return snapshot.message_contains(expected_contains);
    }

    if let Some(screen_id) = step.screen_id {
        if snapshot.screen != screen_id {
            return false;
        }
    }

    if let Some(modal_id) = step.modal_id {
        if snapshot.open_modal != Some(modal_id) {
            return false;
        }
    }

    if let Some(readiness) = step.readiness {
        if snapshot.readiness != readiness {
            return false;
        }
    }
    if let Some(quiescence) = step.quiescence.as_ref() {
        if snapshot.quiescence.state != *quiescence {
            return false;
        }
    }
    if let Some(kind) = step.runtime_event_kind {
        let detail_needle = step.contains.as_deref().or(step.value.as_deref());
        let matched = snapshot.has_runtime_event(kind, detail_needle);
        if !matched {
            return false;
        }
    }

    if let (Some(operation_id), Some(operation_state)) =
        (step.operation_id.as_ref(), step.operation_state)
    {
        let Some(observed_state) = snapshot.operation_state(operation_id) else {
            return false;
        };
        if observed_state != operation_state {
            return false;
        }
    }

    if let Some(control_id) = step.control_id {
        let control_visible = match control_id {
            ControlId::Screen(screen) => snapshot.screen == screen,
            ControlId::List(list) => snapshot.lists.iter().any(|candidate| candidate.id == list),
            ControlId::Modal(modal) => snapshot.open_modal == Some(modal),
            _ => snapshot.focused_control == Some(control_id),
        };
        if !control_visible {
            return false;
        }
    }

    if let Some(list_id) = step.list_id {
        let Some(list) = snapshot
            .lists
            .iter()
            .find(|candidate| candidate.id == list_id)
        else {
            return false;
        };
        if let Some(count) = step.count {
            if list.items.len() != count {
                return false;
            }
        }
        if let Some(item_id) = step.item_id.as_deref() {
            let Some(item) = list.items.iter().find(|item| item.id == item_id) else {
                return false;
            };
            if let Some(confirmation) = step.confirmation {
                if item.confirmation != confirmation {
                    return false;
                }
            }
            if let Some(selection) = snapshot
                .selections
                .iter()
                .find(|selection| selection.list == list_id)
            {
                if selection.item_id != item_id {
                    return false;
                }
            }
        }
    }

    if step.runtime_event_kind.is_none() && (step.contains.is_some() || step.level.is_some()) {
        let expected_level = step
            .level
            .as_deref()
            .map(parse_toast_level)
            .transpose()
            .ok()
            .flatten();
        let Some(expected_contains) = step.contains.as_deref() else {
            return false;
        };
        let matched = snapshot.toasts.iter().any(|toast| {
            let kind_matches = match expected_level {
                Some(ToastLevel::Success) => {
                    toast.kind == aura_app::ui::contract::ToastKind::Success
                }
                Some(ToastLevel::Info) => toast.kind == aura_app::ui::contract::ToastKind::Info,
                Some(ToastLevel::Error) => toast.kind == aura_app::ui::contract::ToastKind::Error,
                None => true,
            };
            kind_matches && toast_contains_matches(expected_contains, &toast.message)
        });
        if !matched {
            return false;
        }
    }

    true
}

fn semantic_wait_matches_for_instance(
    step: &CompatibilityStep,
    snapshot: &UiSnapshot,
    context: &ScenarioContext,
    instance_id: &str,
) -> bool {
    let mut non_operation_step = step.clone();
    non_operation_step.operation_id = None;
    non_operation_step.operation_state = None;
    let needs_channel_runtime_event_match = non_operation_step.contains.is_none()
        && non_operation_step.value.is_none()
        && matches!(
            non_operation_step.runtime_event_kind,
            Some(
                RuntimeEventKind::ChannelMembershipReady
                    | RuntimeEventKind::RecipientPeersResolved
                    | RuntimeEventKind::MessageDeliveryReady
            )
        );
    if needs_channel_runtime_event_match {
        let Some(channel_binding) = context.current_channel_binding.get(instance_id) else {
            return false;
        };
        let mut candidate_step = non_operation_step;
        candidate_step.contains = Some(channel_binding.channel_id.clone());
        if !semantic_wait_matches(&candidate_step, snapshot) {
            return false;
        }
    } else if !semantic_wait_matches(&non_operation_step, snapshot) {
        return false;
    }

    let (Some(operation_id), Some(operation_state)) = (&step.operation_id, step.operation_state)
    else {
        return true;
    };

    let Some(handle) = context.last_operation_handle.get(instance_id) else {
        return true;
    };
    if handle.id() != operation_id {
        return true;
    }

    snapshot.operation_state_for_instance(handle.id(), handle.instance_id())
        == Some(operation_state)
}

fn operation_handle_matches(
    snapshot: &UiSnapshot,
    handle: &UiOperationHandle,
    state: OperationState,
) -> bool {
    snapshot.operation_state_for_instance(handle.id(), handle.instance_id()) == Some(state)
}

fn is_terminal_operation_state(state: OperationState) -> bool {
    matches!(state, OperationState::Succeeded | OperationState::Failed)
}

fn semantic_wait_description(step: &CompatibilityStep) -> String {
    if matches!(step.action, CompatibilityAction::MessageContains) {
        if let Some(value) = step.value.as_deref() {
            return format!("message~={value}");
        }
    }
    if let Some(screen_id) = step.screen_id {
        return format!("screen={}", semantic_screen_name(screen_id));
    }
    if let Some(modal_id) = step.modal_id {
        return format!("modal={}", semantic_modal_name(modal_id));
    }
    if let Some(readiness) = step.readiness {
        return format!("readiness={readiness:?}");
    }
    if let Some(quiescence) = step.quiescence.as_ref() {
        return format!("quiescence={quiescence:?}");
    }
    if let Some(kind) = step.runtime_event_kind {
        return format!("runtime_event={kind:?}");
    }
    if let (Some(operation_id), Some(operation_state)) =
        (step.operation_id.as_ref(), step.operation_state)
    {
        return format!("operation={} state={operation_state:?}", operation_id.0);
    }
    if let Some(control_id) = step.control_id {
        return format!("control={control_id:?}");
    }
    if let Some(contains) = step.contains.as_deref() {
        return format!("toast~={contains}");
    }
    if let Some(list_id) = step.list_id {
        if let Some(count) = step.count {
            return format!("list={list_id:?} count={count}");
        }
        if let Some(item_id) = step.item_id.as_deref() {
            return format!("list={list_id:?} item={item_id}");
        }
        return format!("list={list_id:?}");
    }
    if let Some(peer_instance) = step.peer_instance.as_deref() {
        return format!("parity_with={peer_instance}");
    }
    "semantic state".to_string()
}

fn semantic_screen_name(screen: ScreenId) -> &'static str {
    screen_item_id(screen)
}

fn record_submission_handle(
    context: &mut ScenarioContext,
    instance_id: &str,
    handle: Option<UiOperationHandle>,
) {
    if let Some(handle) = handle {
        context
            .last_operation_handle
            .insert(instance_id.to_string(), handle);
    }
}

fn submit_shared_intent(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    intent: IntentAction,
) -> Result<SemanticCommandResponse> {
    if let Ok(snapshot) = fetch_ui_snapshot(tool_api, instance_id) {
        set_projection_baseline(context, instance_id, snapshot);
    }
    issue_stage(
        step,
        tool_api.submit_semantic_command(instance_id, SemanticCommandRequest::new(intent)),
    )
}

fn set_projection_baseline(context: &mut ScenarioContext, instance_id: &str, snapshot: UiSnapshot) {
    context
        .pending_projection_baseline
        .insert(instance_id.to_string(), snapshot.revision);
    context
        .pending_projection_baseline_snapshot
        .insert(instance_id.to_string(), snapshot);
}

fn clear_projection_baseline(context: &mut ScenarioContext, instance_id: &str) {
    context.pending_projection_baseline.remove(instance_id);
    context
        .pending_projection_baseline_snapshot
        .remove(instance_id);
}

fn clear_projection_baseline_if_semantic_state_already_visible(
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    wait_step: &CompatibilityStep,
) {
    let Ok(snapshot) = fetch_ui_snapshot(tool_api, instance_id) else {
        return;
    };
    if semantic_wait_matches_for_instance(wait_step, &snapshot, context, instance_id) {
        clear_projection_baseline(context, instance_id);
    }
}

fn require_semantic_unit_submission(
    step: &CompatibilityStep,
    operation: &str,
    response: SemanticCommandResponse,
) -> Result<Option<UiOperationHandle>> {
    match response.value {
        SemanticCommandValue::None => Ok(response.handle.ui_operation),
        SemanticCommandValue::ContactInvitationCode { .. } => bail!(
            "step {} issue stage failed for {}: unexpected contact invitation code payload",
            step.id,
            operation
        ),
        SemanticCommandValue::ChannelSelection { .. } => bail!(
            "step {} issue stage failed for {}: unexpected channel selection payload",
            step.id,
            operation
        ),
        SemanticCommandValue::AuthoritativeChannelBinding { .. } => bail!(
            "step {} issue stage failed for {}: unexpected channel binding payload",
            step.id,
            operation
        ),
    }
}

fn require_semantic_unit_submission_with_exact_handle(
    step: &CompatibilityStep,
    operation: &str,
    response: SemanticCommandResponse,
) -> Result<UiOperationHandle> {
    let handle = require_semantic_unit_submission(step, operation, response)?;
    handle.ok_or_else(|| {
        anyhow!(
            "step {} issue stage failed for {}: missing canonical ui operation handle with exact instance tracking",
            step.id,
            operation
        )
    })
}

fn require_channel_binding_submission(
    step: &CompatibilityStep,
    operation: &str,
    response: SemanticCommandResponse,
) -> Result<(ChannelBinding, Option<UiOperationHandle>)> {
    match response.value {
        SemanticCommandValue::AuthoritativeChannelBinding {
            channel_id,
            context_id,
        } => Ok((
            ChannelBinding {
                channel_id,
                context_id,
            },
            response.handle.ui_operation,
        )),
        SemanticCommandValue::None => bail!(
            "step {} issue stage failed for {}: missing channel binding payload",
            step.id,
            operation
        ),
        SemanticCommandValue::ChannelSelection { channel_id } => bail!(
            "step {} issue stage failed for {}: weak selected-channel payload without authoritative context: {}",
            step.id,
            operation,
            channel_id
        ),
        SemanticCommandValue::ContactInvitationCode { .. } => bail!(
            "step {} issue stage failed for {}: unexpected contact invitation code payload",
            step.id,
            operation
        ),
    }
}

fn require_channel_binding_submission_with_exact_handle(
    step: &CompatibilityStep,
    operation: &str,
    response: SemanticCommandResponse,
) -> Result<(ChannelBinding, UiOperationHandle)> {
    let (binding, handle) = require_channel_binding_submission(step, operation, response)?;
    let handle = handle.ok_or_else(|| {
        anyhow!(
            "step {} issue stage failed for {}: missing canonical ui operation handle with exact instance tracking",
            step.id,
            operation
        )
    })?;
    Ok((binding, handle))
}

fn require_contact_invitation_submission(
    step: &CompatibilityStep,
    response: SemanticCommandResponse,
) -> Result<(Option<String>, Option<UiOperationHandle>)> {
    match response.value {
        SemanticCommandValue::ContactInvitationCode { code } => {
            Ok((Some(code), response.handle.ui_operation))
        }
        SemanticCommandValue::None => Ok((None, response.handle.ui_operation)),
        SemanticCommandValue::ChannelSelection { .. } => bail!(
            "step {} issue stage failed for create_contact_invitation: unexpected channel selection payload",
            step.id
        ),
        SemanticCommandValue::AuthoritativeChannelBinding { .. } => bail!(
            "step {} issue stage failed for create_contact_invitation: unexpected channel binding payload",
            step.id
        ),
    }
}

fn extract_invitation_code(snapshot: &UiSnapshot) -> Option<String> {
    snapshot.runtime_events.iter().rev().find_map(|event| {
        if let RuntimeFact::InvitationCodeReady {
            code: Some(code), ..
        } = &event.fact
        {
            return Some(code.clone());
        }
        None
    })
}

fn issue_stage<T>(step: &CompatibilityStep, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        anyhow::anyhow!(
            "step {} issue stage failed for {}: {error:#}",
            step.id,
            step.action
        )
    })
}

fn convergence_stage<T>(step: &CompatibilityStep, label: &str, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        anyhow::anyhow!(
            "step {} convergence stage failed for {} ({label}): {error:#}",
            step.id,
            step.action
        )
    })
}

fn semantic_modal_name(modal: ModalId) -> &'static str {
    match modal {
        ModalId::Help => "help",
        ModalId::CreateInvitation => "create_invitation",
        ModalId::InvitationCode => "invitation_code",
        ModalId::AcceptContactInvitation => "accept_contact_invitation",
        ModalId::AcceptChannelInvitation => "accept_channel_invitation",
        ModalId::CreateHome => "create_home",
        ModalId::CreateChannel => "create_channel",
        ModalId::ChannelInfo => "channel_info",
        ModalId::EditNickname => "edit_nickname",
        ModalId::RemoveContact => "remove_contact",
        ModalId::GuardianSetup => "guardian_setup",
        ModalId::RequestRecovery => "request_recovery",
        ModalId::AddDevice => "add_device",
        ModalId::ImportDeviceEnrollmentCode => "import_device_enrollment_code",
        ModalId::SelectDeviceToRemove => "select_device_to_remove",
        ModalId::ConfirmRemoveDevice => "confirm_remove_device",
        ModalId::MfaSetup => "mfa_setup",
        ModalId::AssignModerator => "assign_moderator",
        ModalId::SwitchAuthority => "switch_authority",
        ModalId::AccessOverride => "access_override",
        ModalId::CapabilityConfig => "capability_config",
        ModalId::EditChannelInfo => "edit_channel_info",
    }
}

fn wait_for_semantic_state(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    timeout_ms: u64,
) -> Result<()> {
    wait_for_semantic_state_snapshot(step, tool_api, context, instance_id, timeout_ms).map(|_| ())
}

fn wait_for_semantic_state_snapshot(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    context: &mut ScenarioContext,
    instance_id: &str,
    timeout_ms: u64,
) -> Result<UiSnapshot> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let supports_ui_snapshot = tool_api.supports_ui_snapshot(instance_id).unwrap_or(false);
    let mut required_newer_than = context
        .pending_projection_baseline
        .get(instance_id)
        .copied();
    let restart_handling = semantic_wait_restart_handling(step);
    let mut snapshot_version = None;
    let mut last_snapshot = loop {
        match fetch_current_ui_snapshot_event(tool_api, instance_id, supports_ui_snapshot) {
            Ok((snapshot, version)) => {
                snapshot_version = version;
                break snapshot;
            }
            Err(error)
                if matches!(
                    classify_browser_ui_snapshot_issue(&error),
                    Some(BrowserUiSnapshotIssue::TransientTimeout)
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                blocking_sleep(Duration::from_millis(100));
            }
            Err(error)
                if matches!(
                    classify_browser_ui_snapshot_issue(&error),
                    Some(BrowserUiSnapshotIssue::BrowserRestarted)
                ) =>
            {
                if restart_handling == SemanticWaitRestartHandling::FailClosed {
                    return Err(error);
                }
                reset_semantic_wait_after_restart(
                    context,
                    instance_id,
                    &mut required_newer_than,
                    &mut snapshot_version,
                );
                if Instant::now() >= deadline {
                    return Err(error);
                }
                blocking_sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    };
    match classify_projection_freshness(required_newer_than, &last_snapshot) {
        ProjectionFreshness::Satisfied => {
            if semantic_wait_matches_for_instance(step, &last_snapshot, context, instance_id) {
                clear_projection_baseline(context, instance_id);
                return Ok(last_snapshot);
            }
        }
        ProjectionFreshness::Restarted { baseline, observed } => {
            if restart_handling == SemanticWaitRestartHandling::FailClosed {
                bail!(
                    "step {} semantic wait observed projection restart on instance {} before freshness was satisfied (baseline={:?} observed={:?})",
                    step.id,
                    instance_id,
                    baseline,
                    observed
                );
            }
            reset_semantic_wait_after_restart(
                context,
                instance_id,
                &mut required_newer_than,
                &mut snapshot_version,
            );
            if semantic_wait_matches_for_instance(step, &last_snapshot, context, instance_id) {
                return Ok(last_snapshot);
            }
        }
        ProjectionFreshness::Pending => {
            if projection_wait_can_resume_from_matching_snapshot(
                step,
                &last_snapshot,
                context,
                instance_id,
                required_newer_than,
                restart_handling,
            ) {
                clear_projection_baseline(context, instance_id);
                return Ok(last_snapshot);
            }
        }
    }
    loop {
        if Instant::now() >= deadline {
            break;
        }
        let snapshot = if supports_ui_snapshot {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match tool_api.wait_for_ui_snapshot_event(instance_id, remaining, snapshot_version) {
                Ok(Some(event)) => {
                    snapshot_version = Some(event.version);
                    event.snapshot
                }
                Ok(None) => match fetch_ui_snapshot(tool_api, instance_id) {
                    Ok(snapshot) => snapshot,
                    Err(error)
                        if matches!(
                            classify_browser_ui_snapshot_issue(&error),
                            Some(BrowserUiSnapshotIssue::TransientTimeout)
                        ) =>
                    {
                        blocking_sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(error) => return Err(error),
                },
                Err(error)
                    if matches!(
                        classify_browser_ui_snapshot_issue(&error),
                        Some(BrowserUiSnapshotIssue::TransientTimeout)
                            | Some(BrowserUiSnapshotIssue::BrowserRestarted)
                    ) =>
                {
                    if matches!(
                        classify_browser_ui_snapshot_issue(&error),
                        Some(BrowserUiSnapshotIssue::BrowserRestarted)
                    ) {
                        if restart_handling == SemanticWaitRestartHandling::FailClosed {
                            return Err(error);
                        }
                        reset_semantic_wait_after_restart(
                            context,
                            instance_id,
                            &mut required_newer_than,
                            &mut snapshot_version,
                        );
                    }
                    match fetch_ui_snapshot(tool_api, instance_id) {
                        Ok(snapshot) => snapshot,
                        Err(fetch_error)
                            if matches!(
                                classify_browser_ui_snapshot_issue(&fetch_error),
                                Some(BrowserUiSnapshotIssue::TransientTimeout)
                                    | Some(BrowserUiSnapshotIssue::BrowserRestarted)
                            ) =>
                        {
                            blocking_sleep(Duration::from_millis(100));
                            continue;
                        }
                        Err(fetch_error) => return Err(fetch_error),
                    }
                }
                Err(error) => match fetch_ui_snapshot(tool_api, instance_id) {
                    Ok(snapshot) => snapshot,
                    Err(_) => return Err(error),
                },
            }
        } else {
            blocking_sleep(Duration::from_millis(40));
            fetch_ui_snapshot(tool_api, instance_id)?
        };
        match classify_projection_freshness(required_newer_than, &snapshot) {
            ProjectionFreshness::Satisfied => {
                if semantic_wait_matches_for_instance(step, &snapshot, context, instance_id) {
                    clear_projection_baseline(context, instance_id);
                    return Ok(snapshot);
                }
            }
            ProjectionFreshness::Restarted { baseline, observed } => {
                if restart_handling == SemanticWaitRestartHandling::FailClosed {
                    bail!(
                        "step {} semantic wait observed projection restart on instance {} before freshness was satisfied (baseline={:?} observed={:?})",
                        step.id,
                        instance_id,
                        baseline,
                        observed
                    );
                }
                reset_semantic_wait_after_restart(
                    context,
                    instance_id,
                    &mut required_newer_than,
                    &mut snapshot_version,
                );
                if semantic_wait_matches_for_instance(step, &snapshot, context, instance_id) {
                    return Ok(snapshot);
                }
            }
            ProjectionFreshness::Pending => {
                if projection_wait_can_resume_from_matching_snapshot(
                    step,
                    &snapshot,
                    context,
                    instance_id,
                    required_newer_than,
                    restart_handling,
                ) {
                    clear_projection_baseline(context, instance_id);
                    return Ok(snapshot);
                }
            }
        }
        consume_projection_baseline(context, instance_id, &snapshot, &mut required_newer_than);
        last_snapshot = snapshot;
    }
    let diagnostic_screen =
        fetch_diagnostic_screen_capture(tool_api, instance_id, ScreenSource::Default)
            .ok()
            .map(|capture| capture.diagnostic_authoritative_screen);
    bail!(
        "step {} semantic wait timed out on instance {} ({}) last_snapshot={:?} diagnostic_screen={:?}",
        step.id,
        instance_id,
        semantic_wait_description(step),
        Some(last_snapshot),
        diagnostic_screen
    )
}

fn fetch_current_ui_snapshot_event(
    tool_api: &mut ToolApi,
    instance_id: &str,
    supports_ui_snapshot: bool,
) -> Result<(UiSnapshot, Option<u64>)> {
    if supports_ui_snapshot {
        match tool_api.wait_for_ui_snapshot_event(instance_id, Duration::from_millis(1), None) {
            Ok(Some(event)) => return Ok((event.snapshot, Some(event.version))),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }

    Ok((fetch_ui_snapshot(tool_api, instance_id)?, None))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionFreshness {
    Satisfied,
    Pending,
    Restarted {
        baseline: ProjectionRevision,
        observed: ProjectionRevision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticWaitRestartHandling {
    FailClosed,
    ResumeAfterRestart,
}

fn semantic_wait_restart_handling(step: &CompatibilityStep) -> SemanticWaitRestartHandling {
    if step.runtime_event_kind.is_some()
        || (step.operation_id.is_some() && step.operation_state.is_some())
        || (step.contains.is_some()
            && step.runtime_event_kind.is_none()
            && !matches!(step.action, CompatibilityAction::MessageContains))
        || step.level.is_some()
    {
        SemanticWaitRestartHandling::FailClosed
    } else {
        SemanticWaitRestartHandling::ResumeAfterRestart
    }
}

fn reset_semantic_wait_after_restart(
    context: &mut ScenarioContext,
    instance_id: &str,
    required_newer_than: &mut Option<ProjectionRevision>,
    snapshot_version: &mut Option<u64>,
) {
    clear_projection_baseline(context, instance_id);
    *required_newer_than = None;
    *snapshot_version = None;
}

fn projection_wait_can_resume_from_matching_snapshot(
    step: &CompatibilityStep,
    snapshot: &UiSnapshot,
    context: &ScenarioContext,
    instance_id: &str,
    required_newer_than: Option<ProjectionRevision>,
    restart_handling: SemanticWaitRestartHandling,
) -> bool {
    restart_handling == SemanticWaitRestartHandling::ResumeAfterRestart
        && required_newer_than.is_some()
        && semantic_wait_matches_for_instance(step, snapshot, context, instance_id)
        && context
            .pending_projection_baseline_snapshot
            .get(instance_id)
            .is_some_and(|baseline_snapshot| baseline_snapshot != snapshot)
}

fn classify_projection_freshness(
    required_newer_than: Option<ProjectionRevision>,
    snapshot: &UiSnapshot,
) -> ProjectionFreshness {
    required_newer_than
        .map(|baseline| {
            if snapshot.revision.is_newer_than(baseline) {
                ProjectionFreshness::Satisfied
            } else if snapshot.revision.semantic_seq < baseline.semantic_seq {
                ProjectionFreshness::Restarted {
                    baseline,
                    observed: snapshot.revision,
                }
            } else {
                ProjectionFreshness::Pending
            }
        })
        .unwrap_or(ProjectionFreshness::Satisfied)
}

fn consume_projection_baseline(
    context: &mut ScenarioContext,
    instance_id: &str,
    snapshot: &UiSnapshot,
    required_newer_than: &mut Option<ProjectionRevision>,
) {
    if context
        .pending_projection_baseline
        .get(instance_id)
        .is_some_and(|baseline| snapshot.revision.is_newer_than(*baseline))
    {
        clear_projection_baseline(context, instance_id);
        *required_newer_than = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserUiSnapshotIssue {
    TransientTimeout,
    BrowserRestarted,
}

fn classify_browser_ui_snapshot_issue(error: &anyhow::Error) -> Option<BrowserUiSnapshotIssue> {
    let message = error.to_string();
    if message.contains("Target page, context or browser has been closed") {
        return Some(BrowserUiSnapshotIssue::BrowserRestarted);
    }
    if message.contains("wait_for_ui_state timed out")
        || message.contains("request:ui_state timed out")
        || message.contains("Playwright driver ui_state timed out")
        || message.contains("ui_state timed out for request")
    {
        return Some(BrowserUiSnapshotIssue::TransientTimeout);
    }
    None
}

fn wait_for_parity(
    step: &CompatibilityStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    peer_instance: &str,
    timeout_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let local_backend_kind = tool_api.backend_kind(instance_id).unwrap_or("unknown");
    let peer_backend_kind = tool_api.backend_kind(peer_instance).unwrap_or("unknown");
    let mut local = fetch_ui_snapshot(tool_api, instance_id)?;
    let mut peer = fetch_ui_snapshot(tool_api, peer_instance)?;
    let mut local_version = None;
    let mut peer_version = None;
    let mut wait_local_next = true;
    let (last_local, last_peer, last_mismatches) = loop {
        let mismatches = uncovered_ui_parity_mismatches(&local, &peer);
        if mismatches.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break (local, peer, mismatches);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if wait_local_next && local_backend_kind == "playwright_browser" {
            if let Some(event) =
                tool_api.wait_for_ui_snapshot_event(instance_id, remaining, local_version)?
            {
                local = event.snapshot;
                local_version = Some(event.version);
            } else {
                blocking_sleep(Duration::from_millis(40));
                local = fetch_ui_snapshot(tool_api, instance_id)?;
            }
        } else if !wait_local_next && peer_backend_kind == "playwright_browser" {
            if let Some(event) =
                tool_api.wait_for_ui_snapshot_event(peer_instance, remaining, peer_version)?
            {
                peer = event.snapshot;
                peer_version = Some(event.version);
            } else {
                blocking_sleep(Duration::from_millis(40));
                peer = fetch_ui_snapshot(tool_api, peer_instance)?;
            }
        } else {
            blocking_sleep(Duration::from_millis(40));
            local = fetch_ui_snapshot(tool_api, instance_id)?;
            peer = fetch_ui_snapshot(tool_api, peer_instance)?;
        }
        if local_backend_kind != "playwright_browser" || !wait_local_next {
            local = fetch_ui_snapshot(tool_api, instance_id)?;
        }
        if peer_backend_kind != "playwright_browser" || wait_local_next {
            peer = fetch_ui_snapshot(tool_api, peer_instance)?;
        }
        wait_local_next = !wait_local_next;
    };
    bail!(
        "step {} parity wait timed out on {} vs {} mismatches={:?} local={:?} peer={:?}",
        step.id,
        instance_id,
        peer_instance,
        last_mismatches,
        last_local,
        last_peer
    )
}

fn diagnostic_screen_contains(
    tool_api: &mut ToolApi,
    instance_id: &str,
    needle: &str,
) -> Result<bool> {
    let capture = dispatch_diagnostic_screen_capture(
        tool_api,
        ToolRequest::Screen {
            instance_id: instance_id.to_string(),
            screen_source: ScreenSource::Default,
        },
    )?;
    let screen = capture.diagnostic_authoritative_screen;
    Ok(screen.contains(needle))
}

fn should_escape_insert_before_send_keys(keys: &str) -> bool {
    let mut chars = keys.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    !matches!(ch, '\n' | '\r' | '\u{1b}' | '\u{08}' | '\u{7f}')
}

fn toast_contains_matches(expected_contains: &str, message: &str) -> bool {
    if message.contains(expected_contains) {
        return true;
    }

    let expected = expected_contains.trim().to_ascii_lowercase();
    let message = message.to_ascii_lowercase();
    if message.contains(&expected) {
        return true;
    }
    match expected.as_str() {
        // Retry flow can legitimately report either no selection or an active retry.
        "no message selected" => message.contains("retrying message"),
        "mfa requires at least 2 devices" => message.contains("requires at least 2 devices"),
        _ => false,
    }
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

fn shared_semantic_raw_ui_request(request: &ToolRequest) -> bool {
    matches!(
        request,
        ToolRequest::SendKeys { .. }
            | ToolRequest::SendKey { .. }
            | ToolRequest::ActivateControl { .. }
            | ToolRequest::ActivateListItem { .. }
            | ToolRequest::CreateContactInvitation { .. }
            | ToolRequest::ClickButton { .. }
            | ToolRequest::FillInput { .. }
            | ToolRequest::FillField { .. }
            | ToolRequest::WaitFor { .. }
    )
}

fn dispatch_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    request: ToolRequest,
) -> Result<()> {
    dispatch_payload_in_lane(tool_api, lane, request).map(|_| ())
}

fn dispatch(tool_api: &mut ToolApi, request: ToolRequest) -> Result<()> {
    dispatch_in_lane(tool_api, ExecutionLane::FrontendConformance, request)
}

fn dispatch_payload_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    request: ToolRequest,
) -> Result<ToolPayload> {
    if matches!(lane, ExecutionLane::SharedSemantic) && shared_semantic_raw_ui_request(&request) {
        bail!(
            "shared semantic lane may not issue raw UI request {request:?}; move this flow to the semantic command plane or frontend-conformance coverage"
        );
    }
    match tool_api.handle_request(request) {
        ToolResponse::Ok { payload } => Ok(payload),
        ToolResponse::Error { message } => Err(anyhow!(message)),
    }
}

fn dispatch_clipboard_payload_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    request: ToolRequest,
) -> Result<ClipboardPayload> {
    match dispatch_payload_in_lane(tool_api, lane, request)? {
        ToolPayload::Clipboard(payload) => Ok(payload),
        payload => bail!("expected clipboard payload, got {payload:?}"),
    }
}

fn dispatch_clipboard_payload(
    tool_api: &mut ToolApi,
    request: ToolRequest,
) -> Result<ClipboardPayload> {
    dispatch_clipboard_payload_in_lane(tool_api, ExecutionLane::FrontendConformance, request)
}

fn dispatch_ui_snapshot_payload_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    request: ToolRequest,
) -> Result<UiSnapshot> {
    match dispatch_payload_in_lane(tool_api, lane, request)? {
        ToolPayload::UiSnapshot(snapshot) => Ok(snapshot),
        payload => bail!("expected ui snapshot payload, got {payload:?}"),
    }
}

fn dispatch_diagnostic_screen_capture_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    request: ToolRequest,
) -> Result<DiagnosticScreenCapture> {
    match dispatch_payload_in_lane(tool_api, lane, request)? {
        ToolPayload::DiagnosticScreenCapture(capture) => Ok(capture),
        payload => bail!("expected diagnostic screen capture payload, got {payload:?}"),
    }
}

fn dispatch_diagnostic_screen_capture(
    tool_api: &mut ToolApi,
    request: ToolRequest,
) -> Result<DiagnosticScreenCapture> {
    dispatch_diagnostic_screen_capture_in_lane(
        tool_api,
        ExecutionLane::FrontendConformance,
        request,
    )
}

fn dispatch_clipboard_text_in_lane(
    tool_api: &mut ToolApi,
    lane: ExecutionLane,
    instance_id: &str,
    text: &str,
) -> Result<()> {
    require_frontend_conformance_lane(lane, instance_id, "paste_clipboard")?;
    if text.chars().count() <= CLIPBOARD_PASTE_CHUNK_CHARS {
        return dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKeys {
                instance_id: instance_id.to_string(),
                keys: text.to_string(),
            },
        );
    }

    let mut chunk = String::with_capacity(CLIPBOARD_PASTE_CHUNK_CHARS);
    let mut chunk_len = 0usize;
    for ch in text.chars() {
        chunk.push(ch);
        chunk_len += 1;
        if chunk_len >= CLIPBOARD_PASTE_CHUNK_CHARS {
            dispatch_in_lane(
                tool_api,
                lane,
                ToolRequest::SendKeys {
                    instance_id: instance_id.to_string(),
                    keys: chunk.clone(),
                },
            )?;
            chunk.clear();
            chunk_len = 0;
            blocking_sleep(Duration::from_millis(CLIPBOARD_PASTE_INTER_CHUNK_DELAY_MS));
        }
    }

    if !chunk.is_empty() {
        dispatch_in_lane(
            tool_api,
            lane,
            ToolRequest::SendKeys {
                instance_id: instance_id.to_string(),
                keys: chunk,
            },
        )?;
    }
    Ok(())
}

fn dispatch_clipboard_text(tool_api: &mut ToolApi, instance_id: &str, text: &str) -> Result<()> {
    dispatch_clipboard_text_in_lane(
        tool_api,
        ExecutionLane::FrontendConformance,
        instance_id,
        text,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::config::{
        CompatibilityAction, CompatibilityStep, InstanceConfig, InstanceMode, RunConfig,
        RunSection, ScreenSource,
    };
    use crate::coordinator::HarnessCoordinator;
    use aura_app::ui::contract::{
        ConfirmationState, FieldId, ListId, ListItemSnapshot, ListSnapshot, OperationId,
        OperationInstanceId, OperationSnapshot, OperationState, RuntimeEventId,
        RuntimeEventSnapshot, ScreenId, SelectionSnapshot, UiReadiness, UiSnapshot,
    };
    use aura_app::ui::scenarios::ScenarioAction;
    use aura_app::ui_contract::{
        next_projection_revision, ChannelFactKey, QuiescenceSnapshot, QuiescenceState,
    };

    #[allow(clippy::disallowed_methods)]
    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "aura-harness-{label}-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|error| panic!("create temp test dir failed: {error}"));
        root
    }

    fn run_report_once(run: &RunConfig, scenario: &ScenarioConfig) -> ScenarioReport {
        let mut tool_api = ToolApi::new(
            HarnessCoordinator::from_run_config(run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = tool_api.start_all() {
            panic!("start_all failed: {error}");
        }
        let report = ScenarioExecutor::new(ExecutionMode::Compatibility)
            .execute(scenario, &mut tool_api)
            .unwrap_or_else(|error| panic!("execute failed: {error}"));
        if let Err(error) = tool_api.stop_all() {
            panic!("stop_all failed: {error}");
        }
        report
    }

    fn test_scenario_config(
        id: &str,
        goal: &str,
        compatibility_steps: Vec<CompatibilityStep>,
    ) -> ScenarioConfig {
        ScenarioConfig {
            schema_version: 1,
            id: id.to_string(),
            goal: goal.to_string(),
            classification: None,
            execution_mode: Some("compatibility".to_string()),
            required_capabilities: vec![],
            compatibility_steps,
            semantic_steps: Vec::new(),
        }
    }

    #[test]
    fn shared_semantic_lane_rejects_raw_ui_requests() {
        for request in [
            ToolRequest::SendKeys {
                instance_id: "alice".to_string(),
                keys: "hello".to_string(),
            },
            ToolRequest::SendKey {
                instance_id: "alice".to_string(),
                key: ToolKey::Enter,
                repeat: 1,
            },
            ToolRequest::ActivateControl {
                instance_id: "alice".to_string(),
                control_id: ControlId::NavChat,
            },
            ToolRequest::ActivateListItem {
                instance_id: "alice".to_string(),
                list_id: ListId::Channels,
                item_id: "channel-1".to_string(),
            },
            ToolRequest::ClickButton {
                instance_id: "alice".to_string(),
                label: "submit".to_string(),
                selector: None,
            },
            ToolRequest::FillInput {
                instance_id: "alice".to_string(),
                selector: "#aura-input".to_string(),
                value: "value".to_string(),
            },
            ToolRequest::FillField {
                instance_id: "alice".to_string(),
                field_id: FieldId::ChatInput,
                value: "value".to_string(),
            },
        ] {
            assert!(
                shared_semantic_raw_ui_request(&request),
                "shared semantic lane must reject raw request {request:?}"
            );
        }
    }

    #[test]
    fn shared_semantic_lane_allows_observation_and_semantic_adjacent_requests() {
        for request in [
            ToolRequest::UiState {
                instance_id: "alice".to_string(),
            },
            ToolRequest::Screen {
                instance_id: "alice".to_string(),
                screen_source: ScreenSource::Default,
            },
            ToolRequest::GetAuthorityId {
                instance_id: "alice".to_string(),
            },
            ToolRequest::DiagnosticListChannels {
                instance_id: "alice".to_string(),
            },
            ToolRequest::DiagnosticCurrentSelection {
                instance_id: "alice".to_string(),
            },
            ToolRequest::ReadClipboard {
                instance_id: "alice".to_string(),
            },
            ToolRequest::Restart {
                instance_id: "alice".to_string(),
            },
        ] {
            assert!(
                !shared_semantic_raw_ui_request(&request),
                "shared semantic lane should allow non-raw request {request:?}"
            );
        }
    }

    #[test]
    fn shared_open_screen_and_settings_use_semantic_submission_not_raw_ui() {
        let source = include_str!("executor.rs");
        let (_, semantic_body) = source
            .split_once("fn execute_semantic_step(")
            .unwrap_or_else(|| panic!("missing execute_semantic_step"));
        let semantic_body = semantic_body
            .split_once("fn semantic_metadata_step(")
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("missing semantic step end"));
        let (open_screen_prefix, open_settings_and_rest) = semantic_body
            .split_once("SemanticAction::Intent(IntentAction::OpenSettingsSection(section)) => {")
            .unwrap_or_else(|| panic!("missing open settings branch"));
        let open_screen_branch = open_screen_prefix
            .split_once("SemanticAction::Intent(IntentAction::OpenScreen { screen, .. }) => {")
            .map(|(_, branch)| branch)
            .unwrap_or_else(|| panic!("missing open screen branch"));
        let open_settings_branch = open_settings_and_rest
            .split_once("SemanticAction::Variables(variable) =>")
            .map(|(branch, _)| branch)
            .unwrap_or_else(|| panic!("missing variables branch after open settings"));
        assert!(
            open_screen_branch.contains("let open_intent = IntentAction::OpenScreen {"),
            "shared OpenScreen branch must submit the semantic intent"
        );
        assert!(
            open_settings_branch.contains("IntentAction::OpenSettingsSection(*section)"),
            "shared OpenSettingsSection branch must submit the semantic intent"
        );
        assert!(
            !open_screen_branch
                .contains("plan_activate_control_request(&instance_id, nav_control_id_for_screen(*screen_id))"),
            "shared OpenScreen branch must not fall back to raw control activation"
        );
        assert!(
            !open_settings_branch.contains(
                "ToolRequest::ActivateListItem {\n                    instance_id: instance_id.clone(),\n                    list_id: ListId::SettingsSections,"
            ),
            "shared OpenSettingsSection branch must not fall back to raw list activation"
        );
    }

    #[test]
    fn canonical_trace_parity_ignores_actor_ids_and_revisions() {
        let local = vec![
            CanonicalTraceEvent::ActionRequested {
                request: SharedActionRequest {
                    actor: ActorId("alice".to_string()),
                    intent: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    },
                    contract: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .contract(),
                },
                observed_revision: Some(UiSnapshot::loading(ScreenId::Chat).revision),
            },
            CanonicalTraceEvent::ActionSucceeded {
                fact: TerminalSuccessFact {
                    handle: SharedActionHandle {
                        action_id: SharedActionId("alice-join".to_string()),
                        actor: ActorId("alice".to_string()),
                        intent: IntentAction::JoinChannel {
                            channel_name: "shared".to_string(),
                        }
                        .kind(),
                        contract: IntentAction::JoinChannel {
                            channel_name: "shared".to_string(),
                        }
                        .contract(),
                        baseline_revision: None,
                    },
                    success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelJoined),
                    observed_revision: None,
                },
            },
        ];
        let peer = vec![
            CanonicalTraceEvent::ActionRequested {
                request: SharedActionRequest {
                    actor: ActorId("bob".to_string()),
                    intent: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    },
                    contract: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .contract(),
                },
                observed_revision: None,
            },
            CanonicalTraceEvent::ActionSucceeded {
                fact: TerminalSuccessFact {
                    handle: SharedActionHandle {
                        action_id: SharedActionId("bob-join".to_string()),
                        actor: ActorId("bob".to_string()),
                        intent: IntentAction::JoinChannel {
                            channel_name: "shared".to_string(),
                        }
                        .kind(),
                        contract: IntentAction::JoinChannel {
                            channel_name: "shared".to_string(),
                        }
                        .contract(),
                        baseline_revision: Some(UiSnapshot::loading(ScreenId::Chat).revision),
                    },
                    success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelJoined),
                    observed_revision: Some(UiSnapshot::loading(ScreenId::Neighborhood).revision),
                },
            },
        ];

        compare_canonical_traces_for_parity(&local, &peer)
            .unwrap_or_else(|error| panic!("trace parity should hold: {error}"));
    }

    #[test]
    fn canonical_trace_parity_rejects_shape_mismatch() {
        let local = vec![CanonicalTraceEvent::ActionSucceeded {
            fact: TerminalSuccessFact {
                handle: SharedActionHandle {
                    action_id: SharedActionId("alice-send".to_string()),
                    actor: ActorId("alice".to_string()),
                    intent: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
                        channel_id: None,
                        context_id: None,
                    }
                    .kind(),
                    contract: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
                        channel_id: None,
                        context_id: None,
                    }
                    .contract(),
                    baseline_revision: None,
                },
                success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::MessageCommitted),
                observed_revision: None,
            },
        }];
        let peer = vec![CanonicalTraceEvent::ActionSucceeded {
            fact: TerminalSuccessFact {
                handle: SharedActionHandle {
                    action_id: SharedActionId("bob-send".to_string()),
                    actor: ActorId("bob".to_string()),
                    intent: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
                        channel_id: None,
                        context_id: None,
                    }
                    .kind(),
                    contract: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
                        channel_id: None,
                        context_id: None,
                    }
                    .contract(),
                    baseline_revision: None,
                },
                success: TerminalSuccessKind::Readiness(UiReadiness::Ready),
                observed_revision: None,
            },
        }];

        let error = compare_canonical_traces_for_parity(&local, &peer)
            .err()
            .unwrap_or_else(|| panic!("trace mismatch must fail"));
        assert!(error.to_string().contains("canonical trace mismatch"));
    }

    #[test]
    fn action_preconditions_fail_diagnostically_before_issue() {
        let snapshot = UiSnapshot::loading(ScreenId::Chat);
        let failures = unsatisfied_action_preconditions(
            &IntentAction::SendChatMessage {
                message: "hello".to_string(),
                channel_id: None,
                context_id: None,
            }
            .contract(),
            &snapshot,
        );
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("readiness=")),
            "expected readiness failure, got {failures:?}"
        );
        assert!(
            failures
                .iter()
                .all(|failure| !failure.contains("quiescence=")),
            "unexpected quiescence failure, got {failures:?}"
        );
        assert!(
            !failures
                .iter()
                .any(|failure| failure.contains("runtime_event=")),
            "unexpected runtime-event failure, got {failures:?}"
        );
    }

    #[test]
    fn action_precondition_wait_step_tracks_all_declared_preconditions() {
        let step = SemanticStep {
            id: "wait-before-remove-device".to_string(),
            action: ScenarioAction::Intent(IntentAction::RemoveSelectedDevice { device_id: None }),
            actor: Some(ActorId("bob".to_string())),
            timeout_ms: Some(4000),
        };
        let wait_step = action_precondition_wait_step(
            &step,
            &IntentAction::RemoveSelectedDevice { device_id: None }.contract(),
        );

        assert!(matches!(wait_step.action, CompatibilityAction::WaitFor));
        assert_eq!(wait_step.screen_id, Some(ScreenId::Settings));
        assert_eq!(wait_step.readiness, Some(UiReadiness::Ready));
        assert_eq!(wait_step.quiescence, Some(QuiescenceState::Settled));
        assert_eq!(wait_step.instance.as_deref(), Some("bob"));
    }

    #[test]
    fn action_precondition_wait_success_returns_without_bailing() {
        let source = include_str!("executor.rs");
        let anchor = "if let Err(wait_error) =\n        wait_for_semantic_state(&wait_step, tool_api, context, &instance_id, timeout_ms)\n    {";
        let start = source
            .find(anchor)
            .unwrap_or_else(|| panic!("missing precondition wait block"));
        let tail = &source[start..];
        let expected_tail = "    Ok(())\n}";
        assert!(
            tail.contains(expected_tail),
            "precondition wait block must return Ok(()) after successful waits"
        );
    }

    #[test]
    fn semantic_wait_can_require_confirmed_list_items() {
        let step = crate::config::CompatibilityStep {
            id: "wait-confirmed-contact".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            list_id: Some(ListId::Contacts),
            item_id: Some("authority-1".to_string()),
            confirmation: Some(ConfirmationState::Confirmed),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: vec![SelectionSnapshot {
                list: ListId::Contacts,
                item_id: "authority-1".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Contacts,
                items: vec![ListItemSnapshot {
                    id: "authority-1".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: false,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(semantic_wait_matches(&step, &snapshot));
    }

    #[test]
    fn semantic_expectation_wait_step_resolves_template_backed_selection_ids() {
        let step = SemanticStep {
            id: "wait-current-authority".to_string(),
            action: ScenarioAction::Expect(Expectation::SelectionIs {
                list: ListId::Authorities,
                item_id: "${alice_authority_id}".to_string(),
            }),
            actor: Some(ActorId("alice".to_string())),
            timeout_ms: Some(4000),
        };
        let mut context = ScenarioContext::default();
        context.vars.insert(
            "alice_authority_id".to_string(),
            "authority-1234".to_string(),
        );

        let wait_step = semantic_expectation_wait_step(
            &step,
            match &step.action {
                ScenarioAction::Expect(expectation) => expectation,
                _ => unreachable!("test step uses an expectation action"),
            },
            &context,
        )
        .unwrap_or_else(|error| panic!("selection wait should resolve templates: {error}"));

        assert_eq!(wait_step.list_id, Some(ListId::Authorities));
        assert_eq!(wait_step.item_id.as_deref(), Some("authority-1234"));
    }

    #[test]
    fn semantic_wait_for_instance_requires_list_count_match() {
        let step = crate::config::CompatibilityStep {
            id: "wait-two-devices".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            list_id: Some(ListId::Devices),
            count: Some(2),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Settings,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: None,
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: vec![ListSnapshot {
                id: ListId::Devices,
                items: vec![ListItemSnapshot {
                    id: "device:current".to_string(),
                    selected: false,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: true,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(!semantic_wait_matches_for_instance(
            &step,
            &snapshot,
            &ScenarioContext::default(),
            "bob"
        ));
    }

    #[test]
    fn semantic_wait_rejects_pending_local_when_confirmed_is_required() {
        let step = crate::config::CompatibilityStep {
            id: "wait-confirmed-contact".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            list_id: Some(ListId::Contacts),
            item_id: Some("authority-1".to_string()),
            confirmation: Some(ConfirmationState::Confirmed),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: vec![SelectionSnapshot {
                list: ListId::Contacts,
                item_id: "authority-1".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Contacts,
                items: vec![ListItemSnapshot {
                    id: "authority-1".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::PendingLocal,
                    is_current: false,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(!semantic_wait_matches(&step, &snapshot));
    }

    #[test]
    fn semantic_wait_can_require_ready_state() {
        let step = crate::config::CompatibilityStep {
            id: "wait-ready".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            readiness: Some(UiReadiness::Ready),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Neighborhood,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(semantic_wait_matches(&step, &snapshot));
    }

    #[test]
    fn semantic_wait_can_require_operation_state() {
        let step = crate::config::CompatibilityStep {
            id: "wait-op".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            operation_id: Some(OperationId::invitation_accept_contact()),
            operation_state: Some(OperationState::Succeeded),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: vec![OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("test-operation-instance".to_string()),
                state: OperationState::Succeeded,
            }],
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(semantic_wait_matches(&step, &snapshot));
    }

    #[test]
    fn semantic_wait_operation_state_uses_recorded_handle_for_instance() {
        let step = crate::config::CompatibilityStep {
            id: "wait-op-handle".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            operation_id: Some(OperationId::invitation_accept_contact()),
            operation_state: Some(OperationState::Succeeded),
            ..Default::default()
        };
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: vec![
                OperationSnapshot {
                    id: OperationId::invitation_accept_contact(),
                    instance_id: OperationInstanceId("stale-instance".to_string()),
                    state: OperationState::Failed,
                },
                OperationSnapshot {
                    id: OperationId::invitation_accept_contact(),
                    instance_id: OperationInstanceId("fresh-instance".to_string()),
                    state: OperationState::Succeeded,
                },
            ],
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let mut context = ScenarioContext::default();
        context.last_operation_handle.insert(
            "alice".to_string(),
            UiOperationHandle::new(
                OperationId::invitation_accept_contact(),
                OperationInstanceId("fresh-instance".to_string()),
            ),
        );

        assert!(
            !semantic_wait_matches(&step, &snapshot),
            "generic operation-id matching should still see the stale first instance"
        );
        assert!(
            semantic_wait_matches_for_instance(&step, &snapshot, &context, "alice"),
            "handle-aware matching must follow the recorded instance instead of the first matching operation id"
        );
    }

    #[test]
    fn operation_handle_match_requires_matching_instance_and_state() {
        let handle = UiOperationHandle::new(
            OperationId::invitation_accept_contact(),
            OperationInstanceId("handle-instance".to_string()),
        );
        let matching_snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: vec![OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("handle-instance".to_string()),
                state: OperationState::Succeeded,
            }],
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let wrong_instance_snapshot = UiSnapshot {
            operations: vec![OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("other-instance".to_string()),
                state: OperationState::Succeeded,
            }],
            ..matching_snapshot.clone()
        };
        let wrong_state_snapshot = UiSnapshot {
            operations: vec![OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("handle-instance".to_string()),
                state: OperationState::Failed,
            }],
            ..matching_snapshot.clone()
        };

        assert!(operation_handle_matches(
            &matching_snapshot,
            &handle,
            OperationState::Succeeded
        ));
        assert!(!operation_handle_matches(
            &wrong_instance_snapshot,
            &handle,
            OperationState::Succeeded
        ));
        assert!(!operation_handle_matches(
            &wrong_state_snapshot,
            &handle,
            OperationState::Succeeded
        ));
    }

    #[test]
    fn escape_insert_guard_only_for_single_non_control_keys() {
        assert!(should_escape_insert_before_send_keys("r"));
        assert!(should_escape_insert_before_send_keys("3"));
        assert!(!should_escape_insert_before_send_keys("\n"));
        assert!(!should_escape_insert_before_send_keys("\u{1b}"));
        assert!(!should_escape_insert_before_send_keys("hi"));
    }

    #[test]
    fn toast_contains_aliases_retry_variants() {
        assert!(toast_contains_matches(
            "No message selected",
            "Retrying message…"
        ));
        assert!(toast_contains_matches(
            "Neighborhood",
            "neighborhood updated"
        ));
        assert!(toast_contains_matches(
            "MFA requires at least 2 devices",
            "Cannot configure multifactor: requires at least 2 devices"
        ));
        assert!(!toast_contains_matches(
            "No message selected",
            "Invitation Created"
        ));
    }

    #[test]
    fn compatibility_and_agent_modes_share_same_transition_path() {
        let temp_root = unique_test_dir("executor-test");

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: Some(temp_root.join("artifacts")),
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(5),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
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

        let scenario = test_scenario_config(
            "executor-smoke",
            "verify transitions",
            vec![CompatibilityStep {
                id: "step-1".to_string(),
                action: CompatibilityAction::LaunchInstances,
                instance: None,
                timeout_ms: None,
                ..Default::default()
            }],
        );

        let mut compatibility_api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = compatibility_api.start_all() {
            panic!("start_all failed: {error}");
        }
        let compatibility = ScenarioExecutor::new(ExecutionMode::Compatibility)
            .execute(&scenario, &mut compatibility_api)
            .unwrap_or_else(|error| panic!("compatibility execute failed: {error}"));
        if let Err(error) = compatibility_api.stop_all() {
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

        assert_eq!(compatibility.states_visited, agent.states_visited);
    }

    #[test]
    fn repeated_runs_with_same_seed_share_same_report_shape() {
        let temp_root = unique_test_dir("determinism-test");

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-determinism".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: Some(temp_root.join("artifacts")),
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(11),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: temp_root,
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
            }],
        };

        let scenario = test_scenario_config(
            "executor-determinism",
            "verify repeated harness determinism",
            vec![CompatibilityStep {
                id: "step-1".to_string(),
                action: CompatibilityAction::LaunchInstances,
                instance: None,
                timeout_ms: None,
                ..Default::default()
            }],
        );

        let first = run_report_once(&run, &scenario);
        let second = run_report_once(&run, &scenario);

        assert_eq!(first.scenario_id, second.scenario_id);
        assert_eq!(first.execution_mode, second.execution_mode);
        assert_eq!(first.states_visited, second.states_visited);
        assert_eq!(first.transitions, second.transitions);
        assert_eq!(first.canonical_trace, second.canonical_trace);
        assert_eq!(first.completed, second.completed);
    }

    #[test]
    fn send_chat_command_dismisses_toast_then_sends_slash_command() {
        let temp_root = unique_test_dir("executor-chat-command");

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-chat-command".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: Some(temp_root.join("artifacts")),
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(7),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
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

        let scenario = test_scenario_config(
            "executor-chat-command",
            "verify chat command action",
            vec![CompatibilityStep {
                id: "step-1".to_string(),
                action: CompatibilityAction::SendChatCommand,
                instance: Some("alice".to_string()),
                command: Some("join slash-lab".to_string()),
                timeout_ms: None,
                ..Default::default()
            }],
        );

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
        {
            panic!("send_chat_command execute failed: {error}");
        }

        if let Err(error) = api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        let action_log = api.action_log();
        let filtered = action_log
            .iter()
            .filter(|record| !matches!(record.request, ToolRequest::UiState { .. }))
            .collect::<Vec<_>>();
        assert!(
            filtered.len() >= 4,
            "expected at least four non-UiState tool actions"
        );

        match &filtered[0].request {
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

        let mut next_index = 1usize;
        if matches!(
            filtered.get(1).map(|record| &record.request),
            Some(ToolRequest::SendKeys { instance_id, keys })
                if instance_id == "alice" && keys == "2"
        ) {
            match &filtered[2].request {
                ToolRequest::WaitFor {
                    instance_id,
                    pattern,
                    timeout_ms: _,
                    ..
                } => {
                    assert_eq!(instance_id, "alice");
                    assert_eq!(pattern, "Channels");
                }
                other => panic!("expected WaitFor after chat nav, got {other:?}"),
            }
            next_index = 3;
        }

        match &filtered[next_index].request {
            ToolRequest::SendKey {
                instance_id,
                key: ToolKey::Esc,
                repeat,
            } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(*repeat, 1);
            }
            other => panic!("expected SendKey(Esc) before command entry, got {other:?}"),
        }

        match &filtered[next_index + 1].request {
            ToolRequest::SendKeys { instance_id, keys } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(keys, "i");
            }
            other => panic!("expected SendKeys for insert mode, got {other:?}"),
        }

        match &filtered[next_index + 2].request {
            ToolRequest::SendKeys { instance_id, keys } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(keys, "/join slash-lab\n");
            }
            other => panic!("expected SendKeys for slash command, got {other:?}"),
        }
    }

    #[test]
    fn tui_semantic_actions_emit_expected_tool_requests() {
        assert!(matches!(
            plan_activate_control_request("alice", ControlId::NavChat),
            ToolRequest::ActivateControl {
                instance_id,
                control_id: ControlId::NavChat,
            } if instance_id == "alice"
        ));
        assert!(matches!(
            plan_fill_field_request("alice", FieldId::ChatInput, "typed-value".to_string()),
            ToolRequest::FillField {
                instance_id,
                field_id: FieldId::ChatInput,
                value,
            } if instance_id == "alice" && value == "typed-value"
        ));
        assert!(matches!(
            plan_dismiss_transient_request("alice"),
            ToolRequest::SendKey {
                instance_id,
                key: ToolKey::Esc,
                repeat: 1,
            } if instance_id == "alice"
        ));
    }

    #[test]
    fn send_chat_message_uses_tui_insert_sequence() {
        let requests = plan_tui_send_chat_message_request("alice", "hello-semantic");
        assert_eq!(requests.len(), 1);
        assert!(matches!(
            &requests[0],
            ToolRequest::SendKeys { instance_id, keys }
            if instance_id == "alice" && keys == "ihello-semantic\n\x1b"
        ));
    }

    #[test]
    fn send_clipboard_retries_until_clipboard_file_is_written() {
        let temp_root = unique_test_dir("executor-send-clipboard");
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
                artifact_dir: Some(temp_root.join("artifacts")),
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(8),
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

        let scenario = test_scenario_config(
            "executor-send-clipboard",
            "verify send_clipboard retry",
            vec![CompatibilityStep {
                id: "step-1".to_string(),
                action: CompatibilityAction::SendClipboard,
                instance: Some("bob".to_string()),
                source_instance: Some("alice".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            }],
        );

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        let clipboard_path = alice_data.join(".harness-transient/clipboard.txt");
        let writer_thread = std::thread::spawn(move || {
            blocking_sleep(std::time::Duration::from_millis(200));
            let _ = std::fs::write(&clipboard_path, "invite-code-123\n");
        });

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
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

    #[test]
    fn send_clipboard_long_payload_is_chunked_and_reassembled() {
        let temp_root = unique_test_dir("executor-send-clipboard-chunked");
        let alice_data = temp_root.join("alice");
        let bob_data = temp_root.join("bob");
        let _ = std::fs::create_dir_all(&alice_data);
        let _ = std::fs::create_dir_all(&bob_data);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-send-clipboard-chunked".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: Some(temp_root.join("artifacts")),
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(9),
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
                    data_dir: alice_data.clone(),
                    device_id: None,
                    bind_address: "127.0.0.1:45021".to_string(),
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
                    bind_address: "127.0.0.1:45022".to_string(),
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

        let scenario = test_scenario_config(
            "executor-send-clipboard-chunked",
            "verify long clipboard payload chunking",
            vec![CompatibilityStep {
                id: "step-1".to_string(),
                action: CompatibilityAction::SendClipboard,
                instance: Some("bob".to_string()),
                source_instance: Some("alice".to_string()),
                timeout_ms: Some(2000),
                ..Default::default()
            }],
        );

        let long_payload = "aura:v1:".to_string()
            + &"x".repeat(CLIPBOARD_PASTE_CHUNK_CHARS * 3 + 7)
            + ":127.0.0.1:41001";

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        let clipboard_path = alice_data.join(".harness-transient/clipboard.txt");
        let _ = std::fs::write(&clipboard_path, format!("{long_payload}\n"));

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
        {
            panic!("send_clipboard execute failed: {error}");
        }

        if let Err(error) = api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        let chunks: Vec<String> = api
            .action_log()
            .iter()
            .filter_map(|entry| match &entry.request {
                ToolRequest::SendKeys { instance_id, keys } if instance_id == "bob" => {
                    Some(keys.clone())
                }
                _ => None,
            })
            .collect();
        assert!(
            chunks.len() > 1,
            "expected long clipboard text to be chunked"
        );
        let reassembled = chunks.join("");
        assert_eq!(reassembled, long_payload);
    }

    #[test]
    fn wait_contract_refs_cover_all_parity_wait_kinds() {
        let modal = WaitContractRef::Modal(ModalId::AddDevice);
        let runtime = WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted);
        let screen = WaitContractRef::Screen(ScreenId::Chat);
        let readiness = WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready);
        let quiescence =
            WaitContractRef::Quiescence(aura_app::ui_contract::QuiescenceState::Settled);
        let operation = WaitContractRef::OperationState {
            operation_id: OperationId::invitation_accept_contact(),
            state: OperationState::Succeeded,
            label: "accept_contact_invitation",
        };

        assert!(matches!(modal, WaitContractRef::Modal(ModalId::AddDevice)));
        assert!(matches!(
            runtime,
            WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted)
        ));
        assert!(matches!(screen, WaitContractRef::Screen(ScreenId::Chat)));
        assert!(matches!(
            readiness,
            WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready)
        ));
        assert!(matches!(
            quiescence,
            WaitContractRef::Quiescence(aura_app::ui_contract::QuiescenceState::Settled)
        ));
        assert!(matches!(
            operation,
            WaitContractRef::OperationState {
                operation_id: _,
                state: OperationState::Succeeded,
                label: "accept_contact_invitation"
            }
        ));
    }

    #[test]
    fn shared_intent_waits_bind_only_to_declared_barriers() {
        let step = crate::config::CompatibilityStep {
            id: "declared-wait".to_string(),
            ..Default::default()
        };
        let start_device_contract = IntentAction::StartDeviceEnrollment {
            device_name: "phone".to_string(),
            code_name: "device_code".to_string(),
            invitee_authority_id: "authority:peer".to_string(),
        }
        .contract();
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::Screen(ScreenId::Settings),
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::OperationState {
                operation_id: OperationId::device_enrollment(),
                state: OperationState::Succeeded,
                label: "start_device_enrollment",
            },
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::RuntimeEvent(RuntimeEventKind::DeviceEnrollmentCodeReady),
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted),
        )
        .is_err());
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::Modal(ModalId::AddDevice),
        )
        .is_err());

        let import_contract = IntentAction::ImportDeviceEnrollmentCode {
            code: "invite".to_string(),
        }
        .contract();
        assert!(ensure_wait_contract_declared(
            &step,
            &import_contract,
            WaitContractRef::Screen(ScreenId::Neighborhood),
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &import_contract,
            WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
        )
        .is_ok());
        assert!(ensure_wait_contract_declared(
            &step,
            &import_contract,
            WaitContractRef::Modal(ModalId::AddDevice),
        )
        .is_err());
    }

    #[test]
    fn semantic_intent_templates_are_resolved_before_submission() {
        let mut context = ScenarioContext::default();
        context.vars.insert(
            "alice_authority_id".to_string(),
            "authority-a2e0c941-1dc2-088e-ffb4-102cb124ac38".to_string(),
        );
        let resolved = resolve_intent_templates(
            &IntentAction::CreateContactInvitation {
                receiver_authority_id: "${alice_authority_id}".to_string(),
                code_name: Some("contact_code".to_string()),
            },
            &context,
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert!(matches!(
            resolved,
            IntentAction::CreateContactInvitation {
                receiver_authority_id,
                code_name,
            } if receiver_authority_id == "authority-a2e0c941-1dc2-088e-ffb4-102cb124ac38"
                && code_name.as_deref() == Some("contact_code")
        ));
    }

    #[test]
    fn join_channel_templates_reject_channel_id_templates() {
        let mut context = ScenarioContext::default();
        context.vars.insert(
            "shared_channel_id".to_string(),
            "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76".to_string(),
        );
        let error = resolve_intent_templates(
            &IntentAction::JoinChannel {
                channel_name: "${shared_channel_id}".to_string(),
            },
            &context,
        )
        .err()
        .unwrap_or_else(|| panic!("channel id template must fail"));

        assert!(
            error
                .to_string()
                .contains("join_channel requires an authoritative shared channel name"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn exact_handle_required_for_parity_critical_submission() {
        let step = crate::config::CompatibilityStep {
            id: "join-channel".to_string(),
            ..Default::default()
        };
        let response = SemanticCommandResponse::accepted_without_value();

        let error =
            require_semantic_unit_submission_with_exact_handle(&step, "join_channel", response)
                .err()
                .unwrap_or_else(|| panic!("missing handle must fail"));

        assert!(
            error
                .to_string()
                .contains("missing canonical ui operation handle with exact instance tracking"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn parity_critical_executor_paths_do_not_fallback_to_runtime_event_waits() {
        let source = include_str!("executor.rs");
        let production_source = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .unwrap_or(source);
        let invite_fallback_label = format!("{}{}", "pending_home_", "invitation_ready");
        let accept_fallback_label = format!("{}{}", "invitation_", "accepted");
        assert!(
            !production_source.contains(&invite_fallback_label),
            "invite_actor_to_channel must not hide missing terminal publication behind readiness fallback"
        );
        assert!(
            !production_source.contains(&accept_fallback_label),
            "accept_pending_channel_invitation must not hide missing terminal publication behind readiness fallback"
        );
        for forbidden in [
            format!("{}{}", "resolve_channel_id_for_", "shared_name("),
            format!("{}{}", "unique_authoritative_", "shared_channel_id("),
            format!("{}{}", "capture_authoritative_", "channel_id("),
            format!("{}{}", "capture_unique_shared_", "channel_id("),
        ] {
            assert!(
                !production_source.contains(&forbidden),
                "executor shared semantic channel flows must not re-materialize channel ids through {forbidden}"
            );
        }
    }

    #[test]
    fn shared_semantic_variable_actions_use_typed_authority_helpers() {
        let source = include_str!("executor.rs");
        let production_source = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .unwrap_or(source);
        assert!(
            production_source.contains("tool_api.prepare_device_enrollment_invitee_authority("),
            "shared semantic executor should use the typed invitee-authority helper"
        );
        assert!(
            production_source.contains("tool_api.current_authority_id("),
            "shared semantic executor should use the typed current-authority helper"
        );
        assert!(
            !production_source.contains(".get(\"authority_id\")"),
            "shared semantic executor must not field-peek authority_id out of raw JSON payloads"
        );
    }

    #[test]
    fn create_account_and_home_wait_for_declared_contract_barriers() {
        let source = include_str!("executor.rs");
        let production_source = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .unwrap_or(source);

        let create_branch = production_source
            .split("IntentAction::CreateAccount { .. } | IntentAction::CreateHome { .. } => {")
            .nth(1)
            .unwrap_or_else(|| panic!("create_account/create_home branch missing"));
        let create_branch = create_branch
            .split("IntentAction::CreateChannel { channel_name } => {")
            .next()
            .unwrap_or(create_branch);

        assert!(
            create_branch.contains("wait_for_contract_barriers("),
            "create_account/create_home branch must converge on shared contract barriers"
        );
    }

    #[test]
    fn projection_freshness_classifies_restart_explicitly() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let snapshot = UiSnapshot {
            revision: ProjectionRevision {
                semantic_seq: 3,
                render_seq: Some(1),
            },
            ..UiSnapshot::loading(ScreenId::Chat)
        };

        assert!(matches!(
            classify_projection_freshness(Some(baseline), &snapshot),
            ProjectionFreshness::Restarted { baseline: observed_baseline, observed }
                if observed_baseline == baseline && observed == snapshot.revision
        ));
    }

    #[test]
    fn projection_freshness_does_not_treat_restart_as_satisfied() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let snapshot = UiSnapshot {
            revision: ProjectionRevision {
                semantic_seq: 6,
                render_seq: Some(9),
            },
            ..UiSnapshot::loading(ScreenId::Chat)
        };

        assert!(!matches!(
            classify_projection_freshness(Some(baseline), &snapshot),
            ProjectionFreshness::Satisfied
        ));
    }

    #[test]
    fn semantic_wait_restart_handling_resumes_projection_based_waits() {
        let step = crate::config::CompatibilityStep {
            id: "screen-ready".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            screen_id: Some(ScreenId::Neighborhood),
            ..Default::default()
        };

        assert_eq!(
            semantic_wait_restart_handling(&step),
            SemanticWaitRestartHandling::ResumeAfterRestart
        );
    }

    #[test]
    fn semantic_wait_restart_handling_fails_closed_for_runtime_events_and_operation_waits() {
        let runtime_event_step = crate::config::CompatibilityStep {
            id: "wait-contact-link".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            runtime_event_kind: Some(RuntimeEventKind::ContactLinkReady),
            ..Default::default()
        };
        let operation_step = crate::config::CompatibilityStep {
            id: "wait-op".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            operation_id: Some(OperationId::create_channel()),
            operation_state: Some(OperationState::Succeeded),
            ..Default::default()
        };

        assert_eq!(
            semantic_wait_restart_handling(&runtime_event_step),
            SemanticWaitRestartHandling::FailClosed
        );
        assert_eq!(
            semantic_wait_restart_handling(&operation_step),
            SemanticWaitRestartHandling::FailClosed
        );
    }

    #[test]
    fn semantic_wait_restart_reset_clears_stale_projection_and_event_versions() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let mut context = ScenarioContext::default();
        context
            .pending_projection_baseline
            .insert("alice".to_string(), baseline);
        let mut required_newer_than = Some(baseline);
        let mut snapshot_version = Some(19);

        reset_semantic_wait_after_restart(
            &mut context,
            "alice",
            &mut required_newer_than,
            &mut snapshot_version,
        );

        assert!(required_newer_than.is_none());
        assert!(snapshot_version.is_none());
        assert!(!context.pending_projection_baseline.contains_key("alice"));
    }

    #[test]
    fn consuming_projection_baseline_clears_live_required_newer_than_state() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let snapshot = UiSnapshot {
            revision: ProjectionRevision {
                semantic_seq: 8,
                render_seq: Some(1),
            },
            ..UiSnapshot::loading(ScreenId::Settings)
        };
        let mut context = ScenarioContext::default();
        context
            .pending_projection_baseline
            .insert("alice".to_string(), baseline);
        let mut required_newer_than = Some(baseline);

        consume_projection_baseline(&mut context, "alice", &snapshot, &mut required_newer_than);

        assert!(required_newer_than.is_none());
        assert!(!context.pending_projection_baseline.contains_key("alice"));
    }

    #[test]
    fn projection_wait_can_resume_when_matching_snapshot_differs_from_baseline() {
        let step = crate::config::CompatibilityStep {
            id: "devices-count-one".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            list_id: Some(ListId::Devices),
            count: Some(1),
            ..Default::default()
        };
        let baseline = UiSnapshot {
            revision: ProjectionRevision {
                semantic_seq: 12,
                render_seq: Some(8),
            },
            screen: ScreenId::Settings,
            lists: vec![ListSnapshot {
                id: ListId::Devices,
                items: vec![
                    ListItemSnapshot {
                        id: "device-a".to_string(),
                        selected: false,
                        confirmation: ConfirmationState::Confirmed,
                        is_current: false,
                    },
                    ListItemSnapshot {
                        id: "device-b".to_string(),
                        selected: false,
                        confirmation: ConfirmationState::Confirmed,
                        is_current: true,
                    },
                ],
            }],
            ..UiSnapshot::loading(ScreenId::Settings)
        };
        let matching_snapshot = UiSnapshot {
            revision: baseline.revision,
            screen: ScreenId::Settings,
            lists: vec![ListSnapshot {
                id: ListId::Devices,
                items: vec![ListItemSnapshot {
                    id: "device-b".to_string(),
                    selected: false,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: true,
                }],
            }],
            ..UiSnapshot::loading(ScreenId::Settings)
        };
        let mut context = ScenarioContext::default();
        set_projection_baseline(&mut context, "alice", baseline);

        assert!(projection_wait_can_resume_from_matching_snapshot(
            &step,
            &matching_snapshot,
            &context,
            "alice",
            Some(matching_snapshot.revision),
            SemanticWaitRestartHandling::ResumeAfterRestart,
        ));
    }

    #[test]
    fn browser_ui_snapshot_issue_classifies_restart_and_timeout() {
        let restart = anyhow!("Target page, context or browser has been closed");
        let timeout = anyhow!("Playwright driver ui_state timed out for request 7");
        let unknown = anyhow!("some other error");

        assert_eq!(
            classify_browser_ui_snapshot_issue(&restart),
            Some(BrowserUiSnapshotIssue::BrowserRestarted)
        );
        assert_eq!(
            classify_browser_ui_snapshot_issue(&timeout),
            Some(BrowserUiSnapshotIssue::TransientTimeout)
        );
        assert_eq!(classify_browser_ui_snapshot_issue(&unknown), None);
    }

    #[test]
    fn semantic_wait_runtime_events_require_authoritative_runtime_facts() {
        let step = crate::config::CompatibilityStep {
            id: "wait-contact-link".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            runtime_event_kind: Some(RuntimeEventKind::ContactLinkReady),
            ..Default::default()
        };
        let mut snapshot = UiSnapshot::loading(ScreenId::Contacts);
        snapshot.lists = vec![ListSnapshot {
            id: ListId::Contacts,
            items: vec![ListItemSnapshot {
                id: "contact-1".to_string(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            }],
        }];

        assert!(
            !semantic_wait_matches(&step, &snapshot),
            "runtime event waits must not fall back to list/UI state"
        );
    }

    #[test]
    fn semantic_wait_channel_runtime_events_require_authoritative_channel_binding_id() {
        let step = crate::config::CompatibilityStep {
            id: "wait-channel-membership".to_string(),
            action: crate::config::CompatibilityAction::WaitFor,
            runtime_event_kind: Some(RuntimeEventKind::ChannelMembershipReady),
            ..Default::default()
        };
        let mut context = ScenarioContext::default();
        context.current_channel_binding.insert(
            "bob".to_string(),
            ChannelBinding {
                channel_id:
                    "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                        .to_string(),
                context_id: "ctx:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                    .to_string(),
            },
        );

        let mut snapshot = UiSnapshot::loading(ScreenId::Chat);
        snapshot.runtime_events.push(RuntimeEventSnapshot {
            id: RuntimeEventId(
                "channel_membership_ready:channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                    .to_string(),
            ),
            fact: RuntimeFact::ChannelMembershipReady {
                channel: ChannelFactKey {
                    id: Some(
                        "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                            .to_string(),
                    ),
                    name: Some("shared-parity-lab".to_string()),
                },
                member_count: Some(2),
            },
        });

        assert!(semantic_wait_matches_for_instance(
            &step, &snapshot, &context, "bob"
        ));
    }

    #[test]
    fn semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks() {
        let source = include_str!("executor.rs");
        for helper in [
            "fn wait_for_semantic_state(",
            "fn wait_for_runtime_event_snapshot(",
            "fn wait_for_operation_handle_state(",
        ] {
            let start = source
                .find(helper)
                .unwrap_or_else(|| panic!("missing helper source for {helper}"));
            let tail = &source[start..];
            let end = tail.find("\nfn ").unwrap_or(tail.len());
            let body = &tail[..end];
            assert!(
                !body.contains("wait_for_diagnostic_dom_patterns("),
                "{helper} must not resolve through DOM pattern fallbacks"
            );
            assert!(
                !body.contains("diagnostic_dom_snapshot("),
                "{helper} must not resolve through raw DOM snapshots"
            );
            assert!(
                !body.contains("diagnostic_screen_contains("),
                "{helper} must not resolve through raw text fallbacks"
            );
            assert!(
                !body.contains("tail_log("),
                "{helper} must not resolve through diagnostic log fallbacks"
            );
        }
    }

    #[test]
    fn raw_text_fallbacks_are_explicitly_diagnostic_only() {
        let source = include_str!("executor.rs");
        let start = source
            .find("fn diagnostic_screen_contains(")
            .unwrap_or_else(|| panic!("missing diagnostic_screen_contains helper"));
        let tail = &source[start..];
        let end = tail.find("\nfn ").unwrap_or(tail.len());
        let body = &tail[..end];
        assert!(!body.contains("FallbackObservationMode"));
    }

    #[test]
    fn diagnostic_capture_paths_do_not_peek_legacy_screen_field_names() {
        let source = include_str!("executor.rs");
        assert!(
            source.contains("DiagnosticScreenCapture"),
            "diagnostic capture helpers must deserialize an explicit diagnostic capture type"
        );
        assert!(
            !source.contains(".get(\"authoritative_screen\")"),
            "executor diagnostics must not peek ambiguous authoritative_screen fields"
        );
        assert!(
            !source.contains(".get(\"raw_screen\")"),
            "executor diagnostics must not peek ambiguous raw_screen fields"
        );
        assert!(
            !source.contains(".get(\"normalized_screen\")"),
            "executor diagnostics must not peek ambiguous normalized_screen fields"
        );
    }
}

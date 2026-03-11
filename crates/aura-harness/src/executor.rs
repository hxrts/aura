//! Scenario step executor for scripted and agent-driven test flows.
//!
//! Interprets scenario steps (input, wait, assert, screenshot) and executes them
//! against backend instances, tracking state transitions and generating reports.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use aura_app::scenario_contract::{
    ActionPrecondition, ActorId, AuthoritativeTransitionFact, AuthoritativeTransitionKind,
    BarrierDeclaration, CanonicalTraceEvent, EnvironmentAction, Expectation, ExtractSource,
    InputKey, IntentAction, SharedActionContract,
    SharedActionHandle, SharedActionId, SharedActionRequest, TerminalFailureFact,
    TerminalSuccessFact, TerminalSuccessKind, ScenarioAction as SemanticAction,
    ScenarioStep as SemanticStep, UiAction, VariableAction,
};
use aura_app::ui::contract::{
    ControlId, FieldId, ListId, ModalId, OperationId, OperationState, RuntimeEventKind, ScreenId,
    ToastKind, UiSnapshot,
};
use aura_app::ui_contract::uncovered_ui_parity_mismatches;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::backend::observe_operation;
use crate::backend::UiOperationHandle;
use crate::config::{
    nav_control_id_for_screen, settings_section_item_id, ScenarioAction, ScenarioConfig,
    ScenarioStep, ScreenSource,
};
use crate::introspection::{
    extract_command_metadata, extract_toast, CommandConsistency, CommandStatus, ToastLevel,
};
use crate::recovery_registry::{run_registered_recovery, RecoveryPath};
use crate::tool_api::{ToolApi, ToolKey, ToolRequest, ToolResponse};

const CLIPBOARD_PASTE_CHUNK_CHARS: usize = 48;
const CLIPBOARD_PASTE_INTER_CHUNK_DELAY_MS: u64 = 5;

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

    fn is_satisfied_by(self, observed: Self) -> bool {
        match self {
            Self::Accepted => {
                matches!(observed, Self::Accepted | Self::Replicated | Self::Enforced)
            }
            Self::Replicated => matches!(observed, Self::Replicated | Self::Enforced),
            Self::Enforced => observed == Self::Enforced,
            Self::PartialTimeout => observed == Self::PartialTimeout,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedCommandStatus {
    Ok,
    Denied,
    Invalid,
    Failed,
}

impl ExpectedCommandStatus {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ok" => Ok(Self::Ok),
            "denied" => Ok(Self::Denied),
            "invalid" => Ok(Self::Invalid),
            "failed" => Ok(Self::Failed),
            other => bail!("unsupported command status value: {other}"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
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

    const fn reason_code(self) -> &'static str {
        match self {
            Self::Permission => "permission_denied",
            Self::Banned => "banned",
            Self::Muted => "muted",
        }
    }
}

#[derive(Debug, Default, Clone)]
struct ScenarioContext {
    vars: BTreeMap<String, String>,
    last_request_id: Option<u64>,
    last_chat_command: BTreeMap<String, String>,
    last_operation_handle: BTreeMap<String, UiOperationHandle>,
    canonical_trace: Vec<CanonicalTraceEvent>,
    shared_flow_state: BTreeMap<String, SharedFlowState>,
    pending_convergence: BTreeMap<String, Vec<BarrierDeclaration>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AccountPhase {
    #[default]
    New,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ContactPhase {
    #[default]
    None,
    InvitationReady,
    Linked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ChannelPhase {
    #[default]
    None,
    InvitationPending,
    MembershipReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MessagingPhase {
    #[default]
    None,
    Ready,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SharedFlowState {
    account: AccountPhase,
    contact: ContactPhase,
    channel: ChannelPhase,
    messaging: MessagingPhase,
}

#[derive(Debug, Clone)]
struct SharedTraceMetadata {
    instance_id: String,
    request: SharedActionRequest,
    handle: SharedActionHandle,
}

struct WaitCoordinator<'a> {
    tool_api: &'a mut ToolApi,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackObservationMode {
    BoundedSecondary,
    DiagnosticOnly,
}

impl<'a> WaitCoordinator<'a> {
    fn new(tool_api: &'a mut ToolApi) -> Self {
        Self { tool_api }
    }

    fn modal(
        &mut self,
        contract: WaitContractRef<'_>,
        step: &ScenarioStep,
        instance_id: &str,
        timeout_ms: u64,
        modal_id: ModalId,
    ) -> Result<()> {
        debug_assert!(matches!(contract, WaitContractRef::Modal(expected) if expected == modal_id));
        wait_for_modal(step, self.tool_api, instance_id, timeout_ms, modal_id)
    }

    fn runtime_event(
        &mut self,
        contract: WaitContractRef<'_>,
        step: &ScenarioStep,
        instance_id: &str,
        timeout_ms: u64,
        kind: RuntimeEventKind,
    ) -> Result<()> {
        debug_assert!(
            matches!(contract, WaitContractRef::RuntimeEvent(expected) if expected == kind)
        );
        wait_for_runtime_event(step, self.tool_api, instance_id, timeout_ms, kind)
    }

    fn semantic_state(
        &mut self,
        contract: WaitContractRef<'_>,
        step: &ScenarioStep,
        instance_id: &str,
        timeout_ms: u64,
    ) -> Result<()> {
        debug_assert!(matches!(
            contract,
            WaitContractRef::Screen(_)
                | WaitContractRef::Readiness(_)
                | WaitContractRef::Quiescence(_)
        ));
        wait_for_semantic_state(step, self.tool_api, instance_id, timeout_ms)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedFlowTransition {
    AccountReady,
    ContactInvitationReady,
    ContactLinked,
    PendingChannelInvitation,
    ChannelMembershipReady,
    MessageVisible,
}

impl SharedFlowState {
    fn apply(self, transition: SharedFlowTransition) -> Result<Self> {
        let mut next = self;
        match transition {
            SharedFlowTransition::AccountReady => {
                if !matches!(self.account, AccountPhase::New) {
                    bail!("account transition AccountReady requires AccountPhase::New");
                }
                next.account = AccountPhase::Ready;
            }
            SharedFlowTransition::ContactInvitationReady => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("contact invitation requires AccountPhase::Ready");
                }
                if !matches!(
                    self.contact,
                    ContactPhase::None | ContactPhase::InvitationReady
                ) {
                    bail!("contact invitation transition requires unlinked contact state");
                }
                next.contact = ContactPhase::InvitationReady;
            }
            SharedFlowTransition::ContactLinked => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("contact link requires AccountPhase::Ready");
                }
                next.contact = ContactPhase::Linked;
            }
            SharedFlowTransition::PendingChannelInvitation => {
                if !matches!(self.contact, ContactPhase::Linked) {
                    bail!("pending channel invitation requires ContactPhase::Linked");
                }
                next.channel = ChannelPhase::InvitationPending;
            }
            SharedFlowTransition::ChannelMembershipReady => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("channel membership requires AccountPhase::Ready");
                }
                next.channel = ChannelPhase::MembershipReady;
                next.messaging = MessagingPhase::Ready;
            }
            SharedFlowTransition::MessageVisible => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("message visibility requires AccountPhase::Ready");
                }
                next.channel = ChannelPhase::MembershipReady;
                next.messaging = MessagingPhase::Visible;
            }
        }
        Ok(next)
    }

    fn allow_action(self, action: ScenarioAction) -> Result<()> {
        match action {
            ScenarioAction::CreateAccount => {
                if !matches!(self.account, AccountPhase::New) {
                    bail!("create_account requires AccountPhase::New");
                }
            }
            ScenarioAction::CreateContactInvitation | ScenarioAction::AcceptContactInvitation => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("{action:?} requires AccountPhase::Ready");
                }
            }
            ScenarioAction::JoinChannel => {
                if !matches!(self.account, AccountPhase::Ready) {
                    bail!("join_channel requires AccountPhase::Ready");
                }
            }
            ScenarioAction::InviteActorToChannel => {
                if !matches!(self.contact, ContactPhase::Linked) {
                    bail!("invite_actor_to_channel requires ContactPhase::Linked");
                }
                if !matches!(self.channel, ChannelPhase::MembershipReady) {
                    bail!("invite_actor_to_channel requires ChannelPhase::MembershipReady");
                }
            }
            ScenarioAction::AcceptPendingChannelInvitation => {
                if !matches!(
                    self.channel,
                    ChannelPhase::None | ChannelPhase::InvitationPending
                ) {
                    bail!(
                        "accept_pending_channel_invitation requires no completed channel membership"
                    );
                }
            }
            ScenarioAction::SendChatMessage => {
                if !matches!(self.channel, ChannelPhase::MembershipReady) {
                    bail!("{action:?} requires ChannelPhase::MembershipReady");
                }
            }
            ScenarioAction::MessageContains => {}
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedSemanticBinding {
    CreateAccount,
    CreateHome,
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    RemoveSelectedDevice,
    CreateContactInvitation,
    AcceptContactInvitation,
    JoinChannel,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
    SendChatMessage,
}

impl SharedSemanticBinding {
    fn from_action(action: ScenarioAction) -> Option<Self> {
        match action {
            ScenarioAction::CreateAccount => Some(Self::CreateAccount),
            ScenarioAction::CreateHome => Some(Self::CreateHome),
            ScenarioAction::StartDeviceEnrollment => Some(Self::StartDeviceEnrollment),
            ScenarioAction::ImportDeviceEnrollmentCode => Some(Self::ImportDeviceEnrollmentCode),
            ScenarioAction::RemoveSelectedDevice => Some(Self::RemoveSelectedDevice),
            ScenarioAction::CreateContactInvitation => Some(Self::CreateContactInvitation),
            ScenarioAction::AcceptContactInvitation => Some(Self::AcceptContactInvitation),
            ScenarioAction::JoinChannel => Some(Self::JoinChannel),
            ScenarioAction::InviteActorToChannel => Some(Self::InviteActorToChannel),
            ScenarioAction::AcceptPendingChannelInvitation => {
                Some(Self::AcceptPendingChannelInvitation)
            }
            ScenarioAction::SendChatMessage => Some(Self::SendChatMessage),
            _ => None,
        }
    }
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
        if let Some(shared_steps) = scenario.shared_execution_semantic_steps() {
            return self.execute_semantic_shared_with_budgets(
                scenario,
                shared_steps,
                tool_api,
                budgets,
            );
        }
        let execution_steps = scenario.execution_steps()?;
        let machine = StateMachine::from_steps(&execution_steps);
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
            let trace_metadata = build_shared_trace_metadata(&state.step, tool_api, &context)?;
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
            let step_result = execute_step(
                &state.step,
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
            canonical_trace: context.canonical_trace,
            step_metrics,
            total_duration_ms: scenario_started.elapsed().as_millis() as u64,
            completed: true,
        })
    }

    fn execute_semantic_shared_with_budgets(
        &self,
        scenario: &ScenarioConfig,
        semantic_steps: &[SemanticStep],
        tool_api: &mut ToolApi,
        budgets: ExecutionBudgets,
    ) -> Result<ScenarioReport> {
        let machine = SemanticStateMachine::from_steps(semantic_steps);
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
            let step_result = execute_semantic_shared_step(
                &state.step,
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

            let next = match self.mode {
                ExecutionMode::Scripted => state.next_state.clone(),
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
            canonical_trace: context.canonical_trace,
            step_metrics,
            total_duration_ms: scenario_started.elapsed().as_millis() as u64,
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

#[derive(Debug, Clone)]
struct SemanticScenarioState {
    id: String,
    step: SemanticStep,
    next_state: Option<String>,
}

#[derive(Debug, Clone)]
struct SemanticStateMachine {
    start_state: Option<String>,
    states: BTreeMap<String, SemanticScenarioState>,
}

impl SemanticStateMachine {
    fn from_steps(steps: &[SemanticStep]) -> Self {
        let mut states = BTreeMap::new();

        for (index, step) in steps.iter().enumerate() {
            let next_state = steps.get(index + 1).map(|step| step.id.clone());
            states.insert(
                step.id.clone(),
                SemanticScenarioState {
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
    if let Some(instance_id) = step.instance.as_deref() {
        ensure_shared_flow_prerequisites(step, context, instance_id)?;
    }
    if let Some(binding) = SharedSemanticBinding::from_action(step.action) {
        let instance_id = resolve_required_instance(step, context)?;
        ensure_post_operation_convergence_satisfied(step, context, &instance_id)?;
        let intent = shared_intent_action(binding, step, context)?;
        enforce_action_preconditions(step, tool_api, context, &intent)?;
        return execute_shared_semantic_action(binding, step, tool_api, step_budget_ms, context);
    }
    match step.action {
        ScenarioAction::LaunchInstances | ScenarioAction::Noop => Ok(()),
        ScenarioAction::CreateHome => {
            unreachable!("shared semantic action should have been handled")
        }
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
        ScenarioAction::CaptureCurrentAuthorityId => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let payload = dispatch_payload(tool_api, ToolRequest::GetAuthorityId { instance_id })?;
            let authority_id = payload
                .get("authority_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "step {} capture_current_authority_id missing authority_id",
                        step.id
                    )
                })?;
            context
                .vars
                .insert(var.to_string(), authority_id.to_string());
            Ok(())
        }
        ScenarioAction::CaptureSelection => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let list_id = step
                .list_id
                .ok_or_else(|| anyhow!("step {} missing list_id", step.id))?;
            let snapshot = fetch_ui_snapshot(tool_api, &instance_id)?;
            let selection = snapshot
                .selections
                .iter()
                .find(|selection| selection.list == list_id)
                .ok_or_else(|| {
                    anyhow!(
                        "step {} capture_selection found no selection for list {:?}",
                        step.id,
                        list_id
                    )
                })?;
            context
                .vars
                .insert(var.to_string(), selection.item_id.clone());
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
                    screen_source: step.screen_source.unwrap_or_default(),
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
            let capture = captures.get(group as usize).ok_or_else(|| {
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
        ScenarioAction::CreateAccount => {
            let instance_id = resolve_required_instance(step, context)?;
            let account_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_create_account(&instance_id, &account_name),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        ScenarioAction::StartDeviceEnrollment => {
            let instance_id = resolve_required_instance(step, context)?;
            let device_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let code_name = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::SettingsAddDeviceButton),
            )?;
            wait_for_modal(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                ModalId::AddDevice,
            )?;
            dispatch(
                tool_api,
                plan_fill_field_request(&instance_id, FieldId::DeviceName, device_name),
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            wait_for_runtime_event(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                RuntimeEventKind::DeviceEnrollmentCodeReady,
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalCopyButton),
            )?;
            let clipboard_text = read_clipboard_value(
                tool_api,
                &instance_id,
                &step.id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            context.vars.insert(code_name.to_string(), clipboard_text);
            Ok(())
        }
        ScenarioAction::ImportDeviceEnrollmentCode => {
            let instance_id = resolve_required_instance(step, context)?;
            let code = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            dispatch(
                tool_api,
                plan_fill_field_request(&instance_id, FieldId::DeviceImportCode, code),
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(
                    &instance_id,
                    ControlId::OnboardingImportDeviceButton,
                ),
            )?;
            let deadline =
                Instant::now() + Duration::from_millis(step.timeout_ms.unwrap_or(step_budget_ms));
            let mut neighborhood_step = semantic_wait_step(step);
            neighborhood_step.screen_id = Some(ScreenId::Neighborhood);
            wait_for_semantic_state(
                &neighborhood_step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .max(1) as u64;
            let mut readiness_step = semantic_wait_step(step);
            readiness_step.readiness = Some(aura_app::ui::contract::UiReadiness::Ready);
            wait_for_semantic_state(&readiness_step, tool_api, &instance_id, remaining_ms)?;
            Ok(())
        }
        ScenarioAction::RemoveSelectedDevice => {
            let instance_id = resolve_required_instance(step, context)?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::SettingsRemoveDeviceButton),
            )?;
            wait_for_modal(
                step,
                tool_api,
                &instance_id,
                timeout_ms,
                ModalId::SelectDeviceToRemove,
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            wait_for_modal(
                step,
                tool_api,
                &instance_id,
                timeout_ms,
                ModalId::ConfirmRemoveDevice,
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            Ok(())
        }
        ScenarioAction::CreateContactInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let receiver_authority_id = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let code_name = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let submission = issue_stage(
                step,
                tool_api.submit_create_contact_invitation(&instance_id, &receiver_authority_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            context
                .vars
                .insert(code_name.to_string(), submission.value.code);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        ScenarioAction::AcceptContactInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let code = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let submission = issue_stage(
                step,
                tool_api.submit_accept_contact_invitation(&instance_id, &code),
            )?;
            let operation_handle = submission.handle.ui_operation.clone();
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            let mut contact_link_step = semantic_wait_step(step);
            contact_link_step.runtime_event_kind = Some(RuntimeEventKind::ContactLinkReady);
            if let Some(handle) = operation_handle {
                let remaining_ms = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1) as u64;
                convergence_stage(
                    step,
                    "accept_contact_operation",
                    wait_for_operation_handle_state(
                        step,
                        tool_api,
                        &instance_id,
                        remaining_ms,
                        &handle,
                        OperationState::Succeeded,
                    ),
                )?;
            }
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .max(1) as u64;
            if convergence_stage(
                step,
                "contact_link",
                wait_for_semantic_state(&contact_link_step, tool_api, &instance_id, remaining_ms),
            )
            .is_err()
            {
                let remaining_ms = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1) as u64;
                let mut contacts_step = semantic_wait_step(step);
                contacts_step.list_id = Some(ListId::Contacts);
                contacts_step.count = Some(1);
                run_registered_recovery(
                    RecoveryPath::AcceptContactInvitationContactsFallback,
                    || {
                        convergence_stage(
                            step,
                            "contacts_list",
                            wait_for_semantic_state(
                                &contacts_step,
                                tool_api,
                                &instance_id,
                                remaining_ms,
                            ),
                        )
                    },
                )?;
            }
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        ScenarioAction::InviteActorToChannel => {
            let instance_id = resolve_required_instance(step, context)?;
            let authority_id = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_invite_actor_to_channel(&instance_id, &authority_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            Ok(())
        }
        ScenarioAction::AcceptPendingChannelInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let submission = issue_stage(
                step,
                tool_api.submit_accept_pending_channel_invitation(&instance_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        ScenarioAction::JoinChannel => {
            let instance_id = resolve_required_instance(step, context)?;
            let channel_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_join_channel(&instance_id, &channel_name),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            Ok(())
        }
        ScenarioAction::SendKeys => {
            let instance_id = resolve_required_instance(step, context)?;
            let keys =
                resolve_optional_field(step.keys.as_deref().or(step.expect.as_deref()), context)?
                    .unwrap_or_else(|| "\n".to_string());
            if should_escape_insert_before_send_keys(&keys)
                && diagnostic_screen_contains(
                    tool_api,
                    &instance_id,
                    "mode: insert",
                    FallbackObservationMode::DiagnosticOnly,
                )
                .unwrap_or(false)
            {
                let _ = dispatch(
                    tool_api,
                    ToolRequest::SendKey {
                        instance_id: instance_id.clone(),
                        key: ToolKey::Esc,
                        repeat: 1,
                    },
                );
            }
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
            let command_body = command.trim_start_matches('/');
            context.last_chat_command.insert(
                instance_id.clone(),
                command_body.trim().to_ascii_lowercase(),
            );

            let backend_kind = tool_api.backend_kind(&instance_id).unwrap_or("unknown");

            if backend_kind != "playwright_browser" {
                // Clear any active toast/modal so command-result waits do not match stale UI.
                let _ = dispatch(
                    tool_api,
                    ToolRequest::SendKey {
                        instance_id: instance_id.clone(),
                        key: ToolKey::Esc,
                        repeat: 1,
                    },
                );
            }
            ensure_chat_screen(step, tool_api, &instance_id, backend_kind, step_budget_ms)?;
            if backend_kind == "playwright_browser" {
                dispatch(
                    tool_api,
                    ToolRequest::FillField {
                        instance_id: instance_id.clone(),
                        field_id: FieldId::ChatInput,
                        value: format!("/{command_body}"),
                    },
                )?;
                dispatch(
                    tool_api,
                    ToolRequest::SendKey {
                        instance_id,
                        key: ToolKey::Enter,
                        repeat: 1,
                    },
                )?;
                return Ok(());
            }
            // First Esc can be consumed by mode normalization; send a second Esc
            // to reliably clear any stale toast before command entry.
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id: instance_id.clone(),
                    key: ToolKey::Esc,
                    repeat: 1,
                },
            )?;
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: instance_id.clone(),
                    keys: "i".to_string(),
                },
            )?;
            std::thread::sleep(Duration::from_millis(180));
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: instance_id.clone(),
                    keys: format!("/{command_body}\n"),
                },
            )?;
            // Browser harness can remain in insert mode after command submit; if so,
            // normalize back to navigation mode so subsequent digit keys switch tabs.
            let snapshot = fetch_ui_snapshot(tool_api, &instance_id).ok();
            if snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.focused_control)
                .or_else(|| {
                    fetch_ui_snapshot(tool_api, &instance_id)
                        .ok()
                        .and_then(|snapshot| snapshot.focused_control)
                })
                == Some(ControlId::Field(FieldId::ChatInput))
            {
                let _ = dispatch(
                    tool_api,
                    ToolRequest::SendKey {
                        instance_id: instance_id.clone(),
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
                    && fetch_ui_snapshot(tool_api, "alice")
                        .ok()
                        .and_then(|snapshot| snapshot.focused_control)
                        != Some(ControlId::Field(FieldId::ChatInput))
                {
                    let _ = dispatch(
                        tool_api,
                        ToolRequest::SendKeys {
                            instance_id: "bob".to_string(),
                            keys: format!("\u{1b}i{action_text}\n"),
                        },
                    );
                }
            }
            Ok(())
        }
        ScenarioAction::SendChatMessage => {
            let instance_id = resolve_required_instance(step, context)?;
            let message = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_send_chat_message(&instance_id, &message),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
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
        ScenarioAction::ReadClipboard => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let text = read_clipboard_value(
                tool_api,
                &instance_id,
                &step.id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            context.vars.insert(var.to_string(), text);
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
        ScenarioAction::DismissTransient => {
            let instance_id = resolve_required_instance(step, context)?;
            dispatch(tool_api, plan_dismiss_transient_request(&instance_id))?;
            Ok(())
        }
        ScenarioAction::ClickButton => {
            let instance_id = resolve_required_instance(step, context)?;
            if let Some(control_id) = step.control_id {
                dispatch(
                    tool_api,
                    plan_activate_control_request(&instance_id, control_id),
                )?;
                return Ok(());
            }
            if let (Some(list_id), Some(item_id)) = (step.list_id, step.item_id.as_deref()) {
                let item_id = resolve_template(item_id, context)?;
                dispatch(
                    tool_api,
                    ToolRequest::ActivateListItem {
                        instance_id,
                        list_id,
                        item_id,
                    },
                )?;
                return Ok(());
            }
            let selector = match step.selector.as_deref() {
                Some(selector) => Some(resolve_template(selector, context)?),
                None => None,
            };
            let label = if selector.is_none() {
                resolve_required_field(
                    step,
                    "label",
                    step.label.as_deref().or(step.expect.as_deref()),
                    context,
                )?
            } else {
                step.label
                    .as_deref()
                    .map(|value| resolve_template(value, context))
                    .transpose()?
                    .unwrap_or_default()
            };
            dispatch(
                tool_api,
                ToolRequest::ClickButton {
                    instance_id,
                    label,
                    selector,
                },
            )?;
            Ok(())
        }
        ScenarioAction::FillInput => {
            let instance_id = resolve_required_instance(step, context)?;
            let value = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            if let Some(field_id) = step.field_id {
                dispatch(
                    tool_api,
                    plan_fill_field_request(&instance_id, field_id, value),
                )?;
            } else {
                let selector =
                    resolve_required_field(step, "selector", step.selector.as_deref(), context)?;
                dispatch(
                    tool_api,
                    ToolRequest::FillInput {
                        instance_id,
                        selector,
                        value,
                    },
                )?;
            }
            Ok(())
        }
        ScenarioAction::AssertParity => {
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
        ScenarioAction::WaitFor | ScenarioAction::MessageContains => {
            let instance_id = resolve_required_instance(step, context)?;
            if matches!(step.action, ScenarioAction::MessageContains)
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
                        .filter(|handle| &handle.id == operation_id)
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
                        &instance_id,
                        step.timeout_ms.unwrap_or(step_budget_ms),
                    ),
                )?;
                record_shared_flow_progress(step, context, &instance_id);
                return Ok(());
            }
            let selector = match step.selector.as_deref() {
                Some(selector) => Some(resolve_template(selector, context)?),
                None => None,
            };
            let pattern = if selector.is_none() {
                resolve_required_field(
                    step,
                    "pattern",
                    step.pattern.as_deref().or(step.expect.as_deref()),
                    context,
                )?
            } else {
                step.pattern
                    .as_deref()
                    .or(step.expect.as_deref())
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
        ScenarioAction::ExpectToast => {
            let instance_id = resolve_required_instance(step, context)?;
            let expected_contains = resolve_required_field(
                step,
                "contains",
                step.contains.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let expected_level = step.level.as_deref().map(parse_toast_level).transpose()?;
            let toast_result = assert_toast(
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
                    toast_contains_matches(&expected_contains, &toast.message)
                },
            );
            if toast_result.is_err()
                && allow_missing_help_toast(
                    &expected_contains,
                    context
                        .last_chat_command
                        .get(&instance_id)
                        .map(String::as_str),
                )
            {
                return Ok(());
            }
            toast_result
        }
        ScenarioAction::ExpectCommandResult => {
            let instance_id = resolve_required_instance(step, context)?;
            let expected_contains = resolve_optional_field(
                step.contains.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let expected_level = step.level.as_deref().map(parse_toast_level).transpose()?;
            let expected_status = step
                .status
                .as_deref()
                .map(ExpectedCommandStatus::parse)
                .transpose()?;
            let expected_consistency = step
                .consistency
                .as_deref()
                .map(ExpectedConsistency::parse)
                .transpose()?;
            let expected_reason_code = step.reason_code.as_deref().map(str::to_ascii_lowercase);
            let toast_result = assert_toast(
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
                        if !command_result_contains_matches(contains, &toast.message) {
                            return false;
                        }
                    }
                    if let Some(status) = expected_status {
                        let Some(found) = extract_command_metadata(&toast.message).status else {
                            return false;
                        };
                        if found.as_str() != status.as_str() {
                            return false;
                        }
                    }
                    if let Some(consistency) = expected_consistency {
                        let Some(found) = extract_command_metadata(&toast.message).consistency
                        else {
                            return false;
                        };
                        let found = match found {
                            CommandConsistency::Accepted => ExpectedConsistency::Accepted,
                            CommandConsistency::Replicated => ExpectedConsistency::Replicated,
                            CommandConsistency::Enforced => ExpectedConsistency::Enforced,
                            CommandConsistency::PartialTimeout => {
                                ExpectedConsistency::PartialTimeout
                            }
                        };
                        if !consistency.is_satisfied_by(found) {
                            return false;
                        }
                    }
                    if let Some(ref reason_code) = expected_reason_code {
                        let Some(found) = extract_command_metadata(&toast.message).reason else {
                            return false;
                        };
                        if found.as_str() != reason_code {
                            return false;
                        }
                    }
                    true
                },
            );
            if toast_result.is_err()
                && allow_missing_command_result_toast(
                    context
                        .last_chat_command
                        .get(&instance_id)
                        .map(String::as_str),
                    expected_status,
                    expected_reason_code.as_deref(),
                    expected_consistency,
                )
            {
                return Ok(());
            }
            toast_result
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
            let expected_status = step
                .status
                .as_deref()
                .map(ExpectedCommandStatus::parse)
                .transpose()?;
            let expected_reason_code = step.reason_code.as_deref().map(str::to_ascii_lowercase);
            let mut contains_any = step.contains_any.clone().unwrap_or_default();
            if let Some(value) = resolve_optional_field(step.contains.as_deref(), context)? {
                contains_any.push(value);
            }
            let toast_result = assert_toast(
                step,
                tool_api,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                |toast| {
                    if toast.level != ToastLevel::Error {
                        return false;
                    }
                    let lowered = toast.message.to_ascii_lowercase();
                    if let Some(expected_status) = expected_status {
                        let Some(found_status) = extract_command_metadata(&toast.message).status
                        else {
                            return false;
                        };
                        if found_status.as_str() != expected_status.as_str() {
                            return false;
                        }
                    } else if let Some(status) = extract_command_metadata(&toast.message).status {
                        if status != CommandStatus::Denied {
                            return false;
                        }
                    }
                    if let Some(reason) = reason {
                        if let Some(found_code) = extract_command_metadata(&toast.message).reason {
                            if found_code.as_str() != reason.reason_code() {
                                return false;
                            }
                        } else if !reason
                            .patterns()
                            .iter()
                            .any(|pattern| lowered.contains(pattern))
                        {
                            return false;
                        }
                    }
                    if let Some(ref reason_code) = expected_reason_code {
                        let Some(found_code) = extract_command_metadata(&toast.message).reason
                        else {
                            return false;
                        };
                        if found_code.as_str() != reason_code {
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
            );
            if toast_result.is_err()
                && allow_missing_denied_toast(
                    context
                        .last_chat_command
                        .get(&instance_id)
                        .map(String::as_str),
                    reason,
                    expected_status,
                    expected_reason_code.as_deref(),
                )
            {
                return Ok(());
            }
            toast_result
        }
        ScenarioAction::GetAuthorityId => {
            let instance_id = resolve_required_instance(step, context)?;
            let var = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let payload = dispatch_payload(tool_api, ToolRequest::GetAuthorityId { instance_id })?;
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
            let payload = dispatch_payload(tool_api, ToolRequest::ListChannels { instance_id })?;
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
            let payload =
                dispatch_payload(tool_api, ToolRequest::CurrentSelection { instance_id })?;
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
            let payload = dispatch_payload(tool_api, ToolRequest::ListContacts { instance_id })?;
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
            let actor = resolve_required_instance(step, context)?;
            tool_api.apply_fault_delay(&actor, delay_ms)
        }
        ScenarioAction::FaultLoss => {
            let actor = resolve_required_instance(step, context)?;
            let _decision = scenario_rng.range_u64(0, 2);
            let loss_percent = step
                .value
                .as_deref()
                .and_then(|value| value.parse::<u8>().ok())
                .unwrap_or(100);
            tool_api.apply_fault_loss(&actor, loss_percent)
        }
        ScenarioAction::FaultTunnelDrop => {
            let actor = resolve_required_instance(step, context)?;
            let _decision = scenario_rng.range_u64(0, 2);
            tool_api.apply_fault_tunnel_drop(&actor)
        }
    }
}

fn execute_semantic_shared_step(
    step: &SemanticStep,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    scenario_rng: &mut DeterministicRng,
    fault_rng: &mut DeterministicRng,
    context: &mut ScenarioContext,
) -> Result<()> {
    let compat_step = semantic_execution_adapter_step(step)?;
    match &step.action {
        SemanticAction::Intent(IntentAction::OpenScreen(screen_id)) => {
            enforce_request_order(&compat_step, context)?;
            let instance_id = resolve_required_instance(&compat_step, context)?;
            ensure_shared_flow_prerequisites(&compat_step, context, &instance_id)?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, nav_control_id_for_screen(*screen_id)),
            )?;
            Ok(())
        }
        SemanticAction::Intent(IntentAction::OpenSettingsSection(section)) => {
            enforce_request_order(&compat_step, context)?;
            let instance_id = resolve_required_instance(&compat_step, context)?;
            ensure_shared_flow_prerequisites(&compat_step, context, &instance_id)?;
            dispatch(
                tool_api,
                ToolRequest::ActivateListItem {
                    instance_id,
                    list_id: ListId::SettingsSections,
                    item_id: settings_section_item_id(*section).to_string(),
                },
            )?;
            Ok(())
        }
        SemanticAction::Variables(variable) => match variable {
            aura_app::scenario_contract::VariableAction::Set { name, value } => {
                let value = resolve_template(value, context)?;
                context.vars.insert(name.clone(), value);
                Ok(())
            }
            aura_app::scenario_contract::VariableAction::CaptureCurrentAuthorityId { name } => {
                let instance_id = resolve_required_instance(&compat_step, context)?;
                let payload =
                    dispatch_payload(tool_api, ToolRequest::GetAuthorityId { instance_id })?;
                let authority_id = payload
                    .get("authority_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        anyhow!(
                            "step {} capture_current_authority_id missing authority_id",
                            step.id
                        )
                    })?;
                context
                    .vars
                    .insert(name.clone(), authority_id.to_string());
                Ok(())
            }
            _ => execute_step(
                &compat_step,
                tool_api,
                step_budget_ms,
                scenario_rng,
                fault_rng,
                context,
            ),
        },
        SemanticAction::Expect(expectation) => {
            let instance_id = resolve_required_instance(&compat_step, context)?;
            match expectation {
                aura_app::scenario_contract::Expectation::ModalOpen(modal_id) => wait_for_modal(
                    &compat_step,
                    tool_api,
                    &instance_id,
                    step_budget_ms,
                    *modal_id,
                ),
                aura_app::scenario_contract::Expectation::RuntimeEventOccurred { kind, .. } => {
                    wait_for_runtime_event(
                        &compat_step,
                        tool_api,
                        &instance_id,
                        step_budget_ms,
                        *kind,
                    )
                }
                aura_app::scenario_contract::Expectation::ParityWithActor { actor } => {
                    wait_for_parity(
                        &compat_step,
                        tool_api,
                        &instance_id,
                        &actor.0,
                        step_budget_ms,
                    )
                }
                _ => wait_for_semantic_state(
                    &compat_step,
                    tool_api,
                    &instance_id,
                    step_budget_ms,
                ),
            }
        }
        _ => execute_step(
            &compat_step,
            tool_api,
            step_budget_ms,
            scenario_rng,
            fault_rng,
            context,
        ),
    }
}

fn semantic_execution_adapter_step(step: &SemanticStep) -> Result<ScenarioStep> {
    if matches!(step.action, SemanticAction::Ui(_)) {
        bail!("canonical shared step {} may not use raw ui mechanics", step.id);
    }

    let SemanticStep {
        id,
        actor,
        timeout_ms,
        action,
    } = step.clone();
    let instance = actor.map(|actor| actor.0);
    let mut compat_step = ScenarioStep {
        id,
        instance,
        timeout_ms,
        ..Default::default()
    };

    match action {
        SemanticAction::Environment(EnvironmentAction::LaunchActors) => {
            compat_step.action = ScenarioAction::LaunchInstances;
        }
        SemanticAction::Environment(EnvironmentAction::RestartActor { actor }) => {
            compat_step.action = ScenarioAction::Restart;
            compat_step.instance = Some(actor.0);
        }
        SemanticAction::Environment(EnvironmentAction::KillActor { actor }) => {
            compat_step.action = ScenarioAction::Kill;
            compat_step.instance = Some(actor.0);
        }
        SemanticAction::Environment(EnvironmentAction::FaultDelay { actor, delay_ms }) => {
            compat_step.action = ScenarioAction::FaultDelay;
            compat_step.instance = Some(actor.0);
            compat_step.timeout_ms = Some(delay_ms);
        }
        SemanticAction::Environment(EnvironmentAction::FaultLoss {
            actor,
            loss_percent,
        }) => {
            compat_step.action = ScenarioAction::FaultLoss;
            compat_step.instance = Some(actor.0);
            compat_step.expect = Some(loss_percent.to_string());
        }
        SemanticAction::Environment(EnvironmentAction::FaultTunnelDrop { actor }) => {
            compat_step.action = ScenarioAction::FaultTunnelDrop;
            compat_step.instance = Some(actor.0);
        }
        SemanticAction::Intent(IntentAction::OpenScreen(screen_id)) => {
            compat_step.action = ScenarioAction::ClickButton;
            compat_step.control_id = Some(nav_control_id_for_screen(screen_id));
        }
        SemanticAction::Intent(IntentAction::CreateAccount { account_name }) => {
            compat_step.action = ScenarioAction::CreateAccount;
            compat_step.value = Some(account_name);
        }
        SemanticAction::Intent(IntentAction::CreateHome { home_name }) => {
            compat_step.action = ScenarioAction::CreateHome;
            compat_step.value = Some(home_name);
        }
        SemanticAction::Intent(IntentAction::StartDeviceEnrollment {
            device_name,
            code_name,
        }) => {
            compat_step.action = ScenarioAction::StartDeviceEnrollment;
            compat_step.value = Some(device_name);
            compat_step.var = Some(code_name);
        }
        SemanticAction::Intent(IntentAction::ImportDeviceEnrollmentCode { code }) => {
            compat_step.action = ScenarioAction::ImportDeviceEnrollmentCode;
            compat_step.value = Some(code);
        }
        SemanticAction::Intent(IntentAction::OpenSettingsSection(section)) => {
            compat_step.action = ScenarioAction::ClickButton;
            compat_step.list_id = Some(ListId::SettingsSections);
            compat_step.item_id = Some(settings_section_item_id(section).to_string());
        }
        SemanticAction::Intent(IntentAction::RemoveSelectedDevice) => {
            compat_step.action = ScenarioAction::RemoveSelectedDevice;
        }
        SemanticAction::Intent(IntentAction::CreateContactInvitation {
            receiver_authority_id,
            code_name,
        }) => {
            compat_step.action = ScenarioAction::CreateContactInvitation;
            compat_step.value = Some(receiver_authority_id);
            compat_step.var = Some(code_name);
        }
        SemanticAction::Intent(IntentAction::AcceptContactInvitation { code }) => {
            compat_step.action = ScenarioAction::AcceptContactInvitation;
            compat_step.value = Some(code);
        }
        SemanticAction::Intent(IntentAction::JoinChannel { channel_name }) => {
            compat_step.action = ScenarioAction::JoinChannel;
            compat_step.value = Some(channel_name);
        }
        SemanticAction::Intent(IntentAction::AcceptPendingChannelInvitation) => {
            compat_step.action = ScenarioAction::AcceptPendingChannelInvitation;
        }
        SemanticAction::Intent(IntentAction::InviteActorToChannel { authority_id }) => {
            compat_step.action = ScenarioAction::InviteActorToChannel;
            compat_step.value = Some(authority_id);
        }
        SemanticAction::Intent(IntentAction::SendChatMessage { message }) => {
            compat_step.action = ScenarioAction::SendChatMessage;
            compat_step.value = Some(message);
        }
        SemanticAction::Ui(UiAction::Navigate(screen_id)) => {
            compat_step.action = ScenarioAction::ClickButton;
            compat_step.control_id = Some(nav_control_id_for_screen(screen_id));
        }
        SemanticAction::Ui(UiAction::Activate(control_id)) => {
            compat_step.action = ScenarioAction::ClickButton;
            compat_step.control_id = Some(control_id);
        }
        SemanticAction::Ui(UiAction::ActivateListItem { list, item_id }) => {
            compat_step.action = ScenarioAction::ClickButton;
            compat_step.list_id = Some(list);
            compat_step.item_id = Some(item_id);
        }
        SemanticAction::Ui(UiAction::Fill(field_id, value)) => {
            compat_step.action = ScenarioAction::FillInput;
            compat_step.field_id = Some(field_id);
            compat_step.value = Some(value);
        }
        SemanticAction::Ui(UiAction::InputText(value)) => {
            compat_step.action = ScenarioAction::SendKeys;
            compat_step.keys = Some(format_input_key_for_compat(value));
        }
        SemanticAction::Ui(UiAction::PressKey(key, repeat)) => {
            compat_step.action = ScenarioAction::SendKey;
            compat_step.key = Some(format_input_key(key));
            compat_step.repeat = Some(repeat);
        }
        SemanticAction::Ui(UiAction::SendChatCommand(command)) => {
            compat_step.action = ScenarioAction::SendChatCommand;
            compat_step.command = Some(command);
        }
        SemanticAction::Ui(UiAction::PasteClipboard { source_actor }) => {
            compat_step.action = ScenarioAction::SendClipboard;
            compat_step.source_instance = source_actor.map(|actor| actor.0);
        }
        SemanticAction::Ui(UiAction::ReadClipboard { name }) => {
            compat_step.action = ScenarioAction::ReadClipboard;
            compat_step.var = Some(name);
        }
        SemanticAction::Ui(UiAction::DismissTransient) => {
            compat_step.action = ScenarioAction::DismissTransient;
        }
        SemanticAction::Expect(Expectation::ScreenIs(screen_id)) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.screen_id = Some(screen_id);
        }
        SemanticAction::Expect(Expectation::ControlVisible(control_id)) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.control_id = Some(control_id);
        }
        SemanticAction::Expect(Expectation::ModalOpen(modal_id)) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.modal_id = Some(modal_id);
        }
        SemanticAction::Expect(Expectation::MessageContains { message_contains }) => {
            compat_step.action = ScenarioAction::MessageContains;
            compat_step.value = Some(message_contains);
        }
        SemanticAction::Expect(Expectation::ToastContains {
            kind,
            message_contains,
        }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.level = kind.map(format_toast_kind);
            compat_step.contains = Some(message_contains);
        }
        SemanticAction::Expect(Expectation::ListContains { list, item_id }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.list_id = Some(list);
            compat_step.item_id = Some(item_id);
        }
        SemanticAction::Expect(Expectation::ListCountIs { list, count }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.list_id = Some(list);
            compat_step.count = Some(count);
        }
        SemanticAction::Expect(Expectation::ListItemConfirmation {
            list,
            item_id,
            confirmation,
        }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.list_id = Some(list);
            compat_step.item_id = Some(item_id);
            compat_step.confirmation = Some(confirmation);
        }
        SemanticAction::Expect(Expectation::SelectionIs { list, item_id }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.list_id = Some(list);
            compat_step.item_id = Some(item_id);
        }
        SemanticAction::Expect(Expectation::ReadinessIs(readiness)) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.readiness = Some(readiness);
        }
        SemanticAction::Expect(Expectation::RuntimeEventOccurred {
            kind,
            detail_contains,
        }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.runtime_event_kind = Some(kind);
            compat_step.contains = detail_contains;
        }
        SemanticAction::Expect(Expectation::OperationStateIs {
            operation_id,
            state,
        }) => {
            compat_step.action = ScenarioAction::WaitFor;
            compat_step.operation_id = Some(operation_id);
            compat_step.operation_state = Some(state);
        }
        SemanticAction::Expect(Expectation::ParityWithActor { actor }) => {
            compat_step.action = ScenarioAction::AssertParity;
            compat_step.peer_instance = Some(actor.0);
        }
        SemanticAction::Variables(VariableAction::Set { name, value }) => {
            compat_step.action = ScenarioAction::SetVar;
            compat_step.var = Some(name);
            compat_step.value = Some(value);
        }
        SemanticAction::Variables(VariableAction::CaptureCurrentAuthorityId { name }) => {
            compat_step.action = ScenarioAction::CaptureCurrentAuthorityId;
            compat_step.var = Some(name);
        }
        SemanticAction::Variables(VariableAction::CaptureSelection { name, list }) => {
            compat_step.action = ScenarioAction::CaptureSelection;
            compat_step.var = Some(name);
            compat_step.list_id = Some(list);
        }
        SemanticAction::Variables(VariableAction::Extract {
            name,
            regex,
            group,
            from,
        }) => {
            compat_step.action = ScenarioAction::ExtractVar;
            compat_step.var = Some(name);
            compat_step.regex = Some(regex);
            compat_step.group = Some(group);
            compat_step.from = Some(format_extract_source(from));
        }
    }

    Ok(compat_step)
}

fn format_input_key(key: InputKey) -> String {
    match key {
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

fn format_input_key_for_compat(value: String) -> String {
    value
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

fn execute_shared_semantic_action(
    binding: SharedSemanticBinding,
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    step_budget_ms: u64,
    context: &mut ScenarioContext,
) -> Result<()> {
    let mut waits = WaitCoordinator::new(tool_api);
    match binding {
        SharedSemanticBinding::CreateAccount => {
            let instance_id = resolve_required_instance(step, context)?;
            let account_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_create_account(&instance_id, &account_name),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        SharedSemanticBinding::CreateHome => {
            let instance_id = resolve_required_instance(step, context)?;
            let home_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission =
                issue_stage(step, tool_api.submit_create_home(&instance_id, &home_name))?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        SharedSemanticBinding::StartDeviceEnrollment => {
            let instance_id = resolve_required_instance(step, context)?;
            let device_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let code_name = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let contract = IntentAction::StartDeviceEnrollment {
                device_name: device_name.clone(),
                code_name: code_name.to_string(),
            }
            .contract();
            dispatch(
                waits.tool_api,
                plan_activate_control_request(&instance_id, ControlId::SettingsAddDeviceButton),
            )?;
            ensure_wait_contract_declared(step, &contract, WaitContractRef::Modal(ModalId::AddDevice))?;
            waits.modal(
                WaitContractRef::Modal(ModalId::AddDevice),
                step,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                ModalId::AddDevice,
            )?;
            dispatch(
                waits.tool_api,
                plan_fill_field_request(&instance_id, FieldId::DeviceName, device_name),
            )?;
            dispatch(
                waits.tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            ensure_wait_contract_declared(
                step,
                &contract,
                WaitContractRef::RuntimeEvent(RuntimeEventKind::DeviceEnrollmentCodeReady),
            )?;
            waits.runtime_event(
                WaitContractRef::RuntimeEvent(RuntimeEventKind::DeviceEnrollmentCodeReady),
                step,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
                RuntimeEventKind::DeviceEnrollmentCodeReady,
            )?;
            dispatch(
                waits.tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalCopyButton),
            )?;
            let clipboard_text = read_clipboard_value(
                waits.tool_api,
                &instance_id,
                &step.id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            context.vars.insert(code_name.to_string(), clipboard_text);
            Ok(())
        }
        SharedSemanticBinding::ImportDeviceEnrollmentCode => {
            let instance_id = resolve_required_instance(step, context)?;
            let code = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let contract = IntentAction::ImportDeviceEnrollmentCode {
                code: code.clone(),
            }
            .contract();
            dispatch(
                waits.tool_api,
                plan_fill_field_request(&instance_id, FieldId::DeviceImportCode, code),
            )?;
            dispatch(
                waits.tool_api,
                plan_activate_control_request(
                    &instance_id,
                    ControlId::OnboardingImportDeviceButton,
                ),
            )?;
            let deadline =
                Instant::now() + Duration::from_millis(step.timeout_ms.unwrap_or(step_budget_ms));
            let mut neighborhood_step = semantic_wait_step(step);
            neighborhood_step.screen_id = Some(ScreenId::Neighborhood);
            ensure_wait_contract_declared(
                step,
                &contract,
                WaitContractRef::Screen(ScreenId::Neighborhood),
            )?;
            waits.semantic_state(
                WaitContractRef::Screen(ScreenId::Neighborhood),
                &neighborhood_step,
                &instance_id,
                step.timeout_ms.unwrap_or(step_budget_ms),
            )?;
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .max(1) as u64;
            let mut readiness_step = semantic_wait_step(step);
            readiness_step.readiness = Some(aura_app::ui::contract::UiReadiness::Ready);
            ensure_wait_contract_declared(
                step,
                &contract,
                WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
            )?;
            waits.semantic_state(
                WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
                &readiness_step,
                &instance_id,
                remaining_ms,
            )?;
            Ok(())
        }
        SharedSemanticBinding::RemoveSelectedDevice => {
            let instance_id = resolve_required_instance(step, context)?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let contract = IntentAction::RemoveSelectedDevice.contract();
            dispatch(
                waits.tool_api,
                plan_activate_control_request(&instance_id, ControlId::SettingsRemoveDeviceButton),
            )?;
            ensure_wait_contract_declared(
                step,
                &contract,
                WaitContractRef::Modal(ModalId::SelectDeviceToRemove),
            )?;
            waits.modal(
                WaitContractRef::Modal(ModalId::SelectDeviceToRemove),
                step,
                &instance_id,
                timeout_ms,
                ModalId::SelectDeviceToRemove,
            )?;
            dispatch(
                waits.tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            ensure_wait_contract_declared(
                step,
                &contract,
                WaitContractRef::Modal(ModalId::ConfirmRemoveDevice),
            )?;
            waits.modal(
                WaitContractRef::Modal(ModalId::ConfirmRemoveDevice),
                step,
                &instance_id,
                timeout_ms,
                ModalId::ConfirmRemoveDevice,
            )?;
            dispatch(
                tool_api,
                plan_activate_control_request(&instance_id, ControlId::ModalConfirmButton),
            )?;
            Ok(())
        }
        SharedSemanticBinding::CreateContactInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let receiver_authority_id = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let code_name = step
                .var
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?;
            let submission = issue_stage(
                step,
                tool_api.submit_create_contact_invitation(&instance_id, &receiver_authority_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            context
                .vars
                .insert(code_name.to_string(), submission.value.code);
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        SharedSemanticBinding::AcceptContactInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let code = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let submission = issue_stage(
                step,
                tool_api.submit_accept_contact_invitation(&instance_id, &code),
            )?;
            let operation_handle = submission.handle.ui_operation.clone();
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            let mut contact_link_step = semantic_wait_step(step);
            contact_link_step.runtime_event_kind = Some(RuntimeEventKind::ContactLinkReady);
            if let Some(handle) = operation_handle {
                let remaining_ms = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1) as u64;
                convergence_stage(
                    step,
                    "accept_contact_operation",
                    wait_for_operation_handle_state(
                        step,
                        tool_api,
                        &instance_id,
                        remaining_ms,
                        &handle,
                        OperationState::Succeeded,
                    ),
                )?;
            }
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .max(1) as u64;
            if convergence_stage(
                step,
                "contact_link",
                wait_for_semantic_state(&contact_link_step, tool_api, &instance_id, remaining_ms),
            )
            .is_err()
            {
                let remaining_ms = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .max(1) as u64;
                let mut contacts_step = semantic_wait_step(step);
                contacts_step.list_id = Some(ListId::Contacts);
                contacts_step.count = Some(1);
                convergence_stage(
                    step,
                    "contacts_list",
                    wait_for_semantic_state(&contacts_step, tool_api, &instance_id, remaining_ms),
                )?;
            }
            record_shared_flow_progress(step, context, &instance_id);
            Ok(())
        }
        SharedSemanticBinding::JoinChannel => {
            let instance_id = resolve_required_instance(step, context)?;
            let channel_name = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_join_channel(&instance_id, &channel_name),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            Ok(())
        }
        SharedSemanticBinding::InviteActorToChannel => {
            let instance_id = resolve_required_instance(step, context)?;
            let authority_id = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let contract = IntentAction::InviteActorToChannel {
                authority_id: authority_id.clone(),
            }
            .contract();
            let submission = issue_stage(
                step,
                tool_api.submit_invite_actor_to_channel(&instance_id, &authority_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            declare_post_operation_convergence(context, &instance_id, &contract);
            Ok(())
        }
        SharedSemanticBinding::AcceptPendingChannelInvitation => {
            let instance_id = resolve_required_instance(step, context)?;
            let contract = IntentAction::AcceptPendingChannelInvitation.contract();
            let submission = issue_stage(
                step,
                tool_api.submit_accept_pending_channel_invitation(&instance_id),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            declare_post_operation_convergence(context, &instance_id, &contract);
            Ok(())
        }
        SharedSemanticBinding::SendChatMessage => {
            let instance_id = resolve_required_instance(step, context)?;
            let message = resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?;
            let submission = issue_stage(
                step,
                tool_api.submit_send_chat_message(&instance_id, &message),
            )?;
            record_submission_handle(context, &instance_id, submission.handle.ui_operation);
            Ok(())
        }
    }
}

fn unsatisfied_action_preconditions(
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
                    "quiescence={:?} expected={expected:?}",
                    snapshot.quiescence.state
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

fn enforce_action_preconditions(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    context: &ScenarioContext,
    intent: &IntentAction,
) -> Result<()> {
    let contract = intent.contract();
    if contract.preconditions.is_empty() {
        return Ok(());
    }
    let instance_id = resolve_required_instance(step, context)?;
    let snapshot = fetch_ui_snapshot(tool_api, &instance_id)?;
    let failures = unsatisfied_action_preconditions(&contract, &snapshot);
    if failures.is_empty() {
        return Ok(());
    }
    bail!(
        "step {} precondition failed before issuing {:?}: {}",
        step.id,
        intent.kind(),
        failures.join(", ")
    );
}

fn wait_contract_matches_barrier(
    contract: &WaitContractRef<'_>,
    barrier: &BarrierDeclaration,
) -> bool {
    match (contract, barrier) {
        (WaitContractRef::Modal(actual), BarrierDeclaration::Modal(expected)) => *actual == *expected,
        (WaitContractRef::RuntimeEvent(actual), BarrierDeclaration::RuntimeEvent(expected)) => {
            *actual == *expected
        }
        (WaitContractRef::Screen(actual), BarrierDeclaration::Screen(expected)) => *actual == *expected,
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

fn ensure_wait_contract_declared(
    step: &ScenarioStep,
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

fn build_shared_trace_metadata(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    context: &ScenarioContext,
) -> Result<Option<SharedTraceMetadata>> {
    let Some(binding) = SharedSemanticBinding::from_action(step.action) else {
        return Ok(None);
    };
    let instance_id = resolve_required_instance(step, context)?;
    let intent = shared_intent_action(binding, step, context)?;
    let contract = intent.contract();
    let baseline_revision = fetch_ui_snapshot(tool_api, &instance_id)
        .ok()
        .map(|snapshot| snapshot.revision);
    let actor = ActorId(instance_id.clone());
    Ok(Some(SharedTraceMetadata {
        instance_id,
        request: SharedActionRequest {
            actor: actor.clone(),
            intent: intent.clone(),
            contract: contract.clone(),
        },
        handle: SharedActionHandle {
            action_id: SharedActionId(step.id.clone()),
            actor,
            intent: intent.kind(),
            contract,
            baseline_revision,
        },
    }))
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
            aura_app::scenario_contract::EnvironmentAction::RestartActor { .. } => {
                "restart_actor"
            }
            aura_app::scenario_contract::EnvironmentAction::KillActor { .. } => "kill_actor",
            aura_app::scenario_contract::EnvironmentAction::FaultDelay { .. } => "fault_delay",
            aura_app::scenario_contract::EnvironmentAction::FaultLoss { .. } => "fault_loss",
            aura_app::scenario_contract::EnvironmentAction::FaultTunnelDrop { .. } => {
                "fault_tunnel_drop"
            }
        },
        SemanticAction::Intent(intent) => match intent {
            IntentAction::OpenScreen(_) => "open_screen",
            IntentAction::CreateAccount { .. } => "create_account",
            IntentAction::CreateHome { .. } => "create_home",
            IntentAction::StartDeviceEnrollment { .. } => "start_device_enrollment",
            IntentAction::ImportDeviceEnrollmentCode { .. } => "import_device_enrollment_code",
            IntentAction::OpenSettingsSection(_) => "open_settings_section",
            IntentAction::RemoveSelectedDevice => "remove_selected_device",
            IntentAction::CreateContactInvitation { .. } => "create_contact_invitation",
            IntentAction::AcceptContactInvitation { .. } => "accept_contact_invitation",
            IntentAction::AcceptPendingChannelInvitation => "accept_pending_channel_invitation",
            IntentAction::JoinChannel { .. } => "join_channel",
            IntentAction::InviteActorToChannel { .. } => "invite_actor_to_channel",
            IntentAction::SendChatMessage { .. } => "send_chat_message",
        },
        SemanticAction::Variables(variable) => match variable {
            aura_app::scenario_contract::VariableAction::Set { .. } => "set_var",
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
            aura_app::scenario_contract::Expectation::MessageContains { .. } => {
                "message_contains"
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
            aura_app::scenario_contract::Expectation::ParityWithActor { .. } => {
                "parity_with_actor"
            }
        },
        SemanticAction::Ui(_) => "ui_mechanic",
    }
}

fn shared_intent_action(
    binding: SharedSemanticBinding,
    step: &ScenarioStep,
    context: &ScenarioContext,
) -> Result<IntentAction> {
    Ok(match binding {
        SharedSemanticBinding::CreateAccount => IntentAction::CreateAccount {
            account_name: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
        SharedSemanticBinding::CreateHome => IntentAction::CreateHome {
            home_name: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
        SharedSemanticBinding::StartDeviceEnrollment => IntentAction::StartDeviceEnrollment {
            device_name: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
            code_name: step
                .var
                .clone()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?,
        },
        SharedSemanticBinding::ImportDeviceEnrollmentCode => {
            IntentAction::ImportDeviceEnrollmentCode {
                code: resolve_required_field(
                    step,
                    "value",
                    step.value.as_deref().or(step.expect.as_deref()),
                    context,
                )?,
            }
        }
        SharedSemanticBinding::RemoveSelectedDevice => IntentAction::RemoveSelectedDevice,
        SharedSemanticBinding::CreateContactInvitation => IntentAction::CreateContactInvitation {
            receiver_authority_id: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
            code_name: step
                .var
                .clone()
                .ok_or_else(|| anyhow!("step {} missing var", step.id))?,
        },
        SharedSemanticBinding::AcceptContactInvitation => IntentAction::AcceptContactInvitation {
            code: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
        SharedSemanticBinding::JoinChannel => IntentAction::JoinChannel {
            channel_name: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
        SharedSemanticBinding::InviteActorToChannel => IntentAction::InviteActorToChannel {
            authority_id: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
        SharedSemanticBinding::AcceptPendingChannelInvitation => {
            IntentAction::AcceptPendingChannelInvitation
        }
        SharedSemanticBinding::SendChatMessage => IntentAction::SendChatMessage {
            message: resolve_required_field(
                step,
                "value",
                step.value.as_deref().or(step.expect.as_deref()),
                context,
            )?,
        },
    })
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
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
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
            wait_step.action = ScenarioAction::WaitFor;
            wait_step.screen_id = Some(ScreenId::Chat);
            wait_step.modal_id = None;
            wait_step.list_id = None;
            wait_step.item_id = None;
            wait_step.operation_id = None;
            wait_step.operation_state = None;
            wait_for_semantic_state(&wait_step, tool_api, instance_id, chat_enter_timeout)
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
    step: &ScenarioStep,
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

#[cfg(test)]
fn plan_tui_send_chat_message_request(instance_id: &str, message: &str) -> Vec<ToolRequest> {
    vec![ToolRequest::SendKeys {
        instance_id: instance_id.to_string(),
        keys: format!("i{message}\n"),
    }]
}

fn semantic_wait_step(step: &ScenarioStep) -> ScenarioStep {
    let mut wait_step = step.clone();
    wait_step.action = ScenarioAction::WaitFor;
    wait_step.expect = None;
    wait_step.keys = None;
    wait_step.screen_source = None;
    wait_step.command = None;
    wait_step.pattern = None;
    wait_step.key = None;
    wait_step.label = None;
    wait_step.selector = None;
    wait_step.screen_id = None;
    wait_step.control_id = None;
    wait_step.field_id = None;
    wait_step.modal_id = None;
    wait_step.readiness = None;
    wait_step.operation_id = None;
    wait_step.operation_state = None;
    wait_step.list_id = None;
    wait_step.item_id = None;
    wait_step.count = None;
    wait_step.confirmation = None;
    wait_step.repeat = None;
    wait_step.source_instance = None;
    wait_step.peer_instance = None;
    wait_step.contains = step.contains.clone();
    wait_step.level = None;
    wait_step.status = None;
    wait_step.consistency = None;
    wait_step.reason_code = None;
    wait_step.channel = None;
    wait_step.selected = None;
    wait_step.present = None;
    wait_step.reason = None;
    wait_step.contains_any = step.contains_any.clone();
    wait_step
}

fn wait_for_modal(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    timeout_ms: u64,
    modal_id: ModalId,
) -> Result<()> {
    let mut wait_step = semantic_wait_step(step);
    wait_step.modal_id = Some(modal_id);
    wait_for_semantic_state(&wait_step, tool_api, instance_id, timeout_ms)
}

fn wait_for_runtime_event(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    timeout_ms: u64,
    runtime_event_kind: RuntimeEventKind,
) -> Result<()> {
    let mut wait_step = semantic_wait_step(step);
    wait_step.runtime_event_kind = Some(runtime_event_kind);
    wait_for_semantic_state(&wait_step, tool_api, instance_id, timeout_ms)
}

fn wait_for_operation_handle_state(
    step: &ScenarioStep,
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
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
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
        handle.id.0,
        handle.instance_id.0,
        Some(last_snapshot)
    )
}

fn read_clipboard_value(
    tool_api: &mut ToolApi,
    instance_id: &str,
    step_id: &str,
    timeout_ms: u64,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let attempt = dispatch_payload(
            tool_api,
            ToolRequest::ReadClipboard {
                instance_id: instance_id.to_string(),
            },
        );
        if let Ok(payload) = attempt {
            if let Some(text) = payload.get("text").and_then(serde_json::Value::as_str) {
                if !text.trim().is_empty() {
                    return Ok(text.to_string());
                }
            }
        }

        if Instant::now() >= deadline {
            bail!("step {step_id} read_clipboard timed out on instance {instance_id} after {timeout_ms}ms");
        }
        std::thread::sleep(Duration::from_millis(100));
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

fn fetch_ui_snapshot(tool_api: &mut ToolApi, instance_id: &str) -> Result<UiSnapshot> {
    let payload = dispatch_payload(
        tool_api,
        ToolRequest::UiState {
            instance_id: instance_id.to_string(),
        },
    )?;
    serde_json::from_value(payload).map_err(Into::into)
}

fn fetch_screen_text(
    tool_api: &mut ToolApi,
    instance_id: &str,
    screen_source: ScreenSource,
) -> Result<String> {
    let payload = dispatch_payload(
        tool_api,
        ToolRequest::Screen {
            instance_id: instance_id.to_string(),
            screen_source,
        },
    )?;
    Ok(payload
        .get("authoritative_screen")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            payload
                .get("normalized_screen")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| payload.get("screen").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .or_else(|| payload.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| payload.to_string()))
}

fn semantic_wait_matches(step: &ScenarioStep, snapshot: &UiSnapshot) -> bool {
    if matches!(step.action, ScenarioAction::MessageContains) {
        let Some(expected_contains) = step.value.as_deref().or(step.expect.as_deref()) else {
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
    if let Some(kind) = step.runtime_event_kind {
        let detail_needle = step
            .contains
            .as_deref()
            .or(step.value.as_deref())
            .or(step.expect.as_deref());
        let matched = snapshot.has_runtime_event(kind, detail_needle)
            || matches!(kind, RuntimeEventKind::ChannelMembershipReady)
                && snapshot
                    .lists
                    .iter()
                    .find(|candidate| candidate.id == ListId::Channels)
                    .map(|channels| {
                        channels.items.iter().any(|item| {
                            item.selected
                                && detail_needle
                                    .map(|needle| item.id.contains(needle))
                                    .unwrap_or(true)
                        })
                    })
                    .unwrap_or(false);
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
        let Some(expected_contains) = step.contains.as_deref().or(step.expect.as_deref()) else {
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

fn operation_handle_matches(
    snapshot: &UiSnapshot,
    handle: &UiOperationHandle,
    state: OperationState,
) -> bool {
    snapshot.operation_state_for_instance(&handle.id, &handle.instance_id) == Some(state)
}

fn semantic_wait_description(step: &ScenarioStep) -> String {
    if matches!(step.action, ScenarioAction::MessageContains) {
        if let Some(value) = step.value.as_deref().or(step.expect.as_deref()) {
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
    if let Some(contains) = step.contains.as_deref().or(step.expect.as_deref()) {
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
    match screen {
        ScreenId::Onboarding => "onboarding",
        ScreenId::Neighborhood => "neighborhood",
        ScreenId::Chat => "chat",
        ScreenId::Contacts => "contacts",
        ScreenId::Notifications => "notifications",
        ScreenId::Settings => "settings",
    }
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

fn issue_stage<T>(step: &ScenarioStep, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        anyhow::anyhow!(
            "step {} issue stage failed for {}: {error:#}",
            step.id,
            step.action
        )
    })
}

fn convergence_stage<T>(step: &ScenarioStep, label: &str, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        anyhow::anyhow!(
            "step {} convergence stage failed for {} ({label}): {error:#}",
            step.id,
            step.action
        )
    })
}

fn shared_flow_state_mut<'a>(
    context: &'a mut ScenarioContext,
    instance_id: &str,
) -> &'a mut SharedFlowState {
    context
        .shared_flow_state
        .entry(instance_id.to_string())
        .or_default()
}

fn pending_convergence_mut<'a>(
    context: &'a mut ScenarioContext,
    instance_id: &str,
) -> &'a mut Vec<BarrierDeclaration> {
    context
        .pending_convergence
        .entry(instance_id.to_string())
        .or_default()
}

fn ensure_post_operation_convergence_satisfied(
    step: &ScenarioStep,
    context: &ScenarioContext,
    instance_id: &str,
) -> Result<()> {
    let Some(pending) = context.pending_convergence.get(instance_id) else {
        return Ok(());
    };
    if pending.is_empty() {
        return Ok(());
    }
    bail!(
        "step {} convergence-contract violation for {:?}: pending {:?}",
        step.id,
        step.action,
        pending
    );
}

fn declare_post_operation_convergence(
    context: &mut ScenarioContext,
    instance_id: &str,
    contract: &SharedActionContract,
) {
    let Some(convergence) = contract.post_operation_convergence.as_ref() else {
        return;
    };
    let pending = pending_convergence_mut(context, instance_id);
    pending.extend(convergence.required_before_next_intent.iter().cloned());
}

fn clear_satisfied_post_operation_convergence(
    context: &mut ScenarioContext,
    instance_id: &str,
    step: &ScenarioStep,
) {
    let Some(pending) = context.pending_convergence.get_mut(instance_id) else {
        return;
    };
    pending.retain(|required| !step_satisfies_barrier(step, required));
    if pending.is_empty() {
        context.pending_convergence.remove(instance_id);
    }
}

fn step_satisfies_barrier(step: &ScenarioStep, barrier: &BarrierDeclaration) -> bool {
    match (step.action, barrier) {
        (ScenarioAction::WaitFor, BarrierDeclaration::RuntimeEvent(expected)) => {
            step.runtime_event_kind == Some(*expected)
        }
        (ScenarioAction::WaitFor, BarrierDeclaration::Readiness(expected)) => {
            step.readiness == Some(*expected)
        }
        (ScenarioAction::WaitFor, BarrierDeclaration::Screen(expected)) => {
            step.screen_id == Some(*expected)
        }
        (ScenarioAction::WaitFor, BarrierDeclaration::Quiescence(_)) => false,
        _ => false,
    }
}

fn ensure_shared_flow_prerequisites(
    step: &ScenarioStep,
    context: &mut ScenarioContext,
    instance_id: &str,
) -> Result<()> {
    let state = *shared_flow_state_mut(context, instance_id);
    state.allow_action(step.action).map_err(|error| {
        anyhow!(
            "step {} invalid shared-flow transition for {:?}: {error}",
            step.id,
            step.action
        )
    })
}

fn record_shared_flow_progress(
    step: &ScenarioStep,
    context: &mut ScenarioContext,
    instance_id: &str,
) {
    clear_satisfied_post_operation_convergence(context, instance_id, step);
    let state = shared_flow_state_mut(context, instance_id);
    let transition = match step.action {
        ScenarioAction::CreateAccount => Some(SharedFlowTransition::AccountReady),
        ScenarioAction::CreateContactInvitation => {
            Some(SharedFlowTransition::ContactInvitationReady)
        }
        ScenarioAction::AcceptContactInvitation => Some(SharedFlowTransition::ContactLinked),
        ScenarioAction::AcceptPendingChannelInvitation => {
            Some(SharedFlowTransition::ChannelMembershipReady)
        }
        ScenarioAction::WaitFor => match step.runtime_event_kind {
            Some(RuntimeEventKind::ContactLinkReady) => Some(SharedFlowTransition::ContactLinked),
            Some(RuntimeEventKind::PendingHomeInvitationReady) => {
                Some(SharedFlowTransition::PendingChannelInvitation)
            }
            Some(RuntimeEventKind::ChannelMembershipReady) => {
                Some(SharedFlowTransition::ChannelMembershipReady)
            }
            _ => None,
        },
        ScenarioAction::MessageContains => Some(SharedFlowTransition::MessageVisible),
        _ => None,
    };
    if let Some(transition) = transition {
        if let Ok(next) = state.apply(transition) {
            *state = next;
        }
    }
}

fn semantic_modal_name(modal: ModalId) -> &'static str {
    match modal {
        ModalId::Help => "help",
        ModalId::CreateInvitation => "create_invitation",
        ModalId::InvitationCode => "invitation_code",
        ModalId::AcceptInvitation => "accept_invitation",
        ModalId::CreateHome => "create_home",
        ModalId::CreateChannel => "create_channel",
        ModalId::SetChannelTopic => "set_channel_topic",
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
    }
}

fn wait_for_semantic_state(
    step: &ScenarioStep,
    tool_api: &mut ToolApi,
    instance_id: &str,
    timeout_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let backend_kind = tool_api.backend_kind(instance_id).unwrap_or("unknown");
    let mut last_snapshot = fetch_ui_snapshot(tool_api, instance_id)?;
    if semantic_wait_matches(step, &last_snapshot) {
        return Ok(());
    }
    let mut browser_version = None;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        let snapshot = if backend_kind == "playwright_browser" {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match tool_api.wait_for_ui_snapshot_event(instance_id, remaining, browser_version) {
                Ok(Some((snapshot, version))) => {
                    browser_version = Some(version);
                    snapshot
                }
                Ok(None) => match fetch_ui_snapshot(tool_api, instance_id) {
                    Ok(snapshot) => snapshot,
                    Err(error) if is_browser_ui_snapshot_transient(&error) => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(error) => return Err(error),
                },
                Err(error) if is_browser_ui_snapshot_transient(&error) => {
                    match fetch_ui_snapshot(tool_api, instance_id) {
                        Ok(snapshot) => snapshot,
                        Err(fetch_error) if is_browser_ui_snapshot_transient(&fetch_error) => {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        Err(fetch_error) => return Err(fetch_error),
                    }
                }
                Err(error) => return Err(error),
            }
        } else {
            std::thread::sleep(Duration::from_millis(40));
            fetch_ui_snapshot(tool_api, instance_id)?
        };
        if semantic_wait_matches(step, &snapshot) {
            return Ok(());
        }
        last_snapshot = snapshot;
    }
    let diagnostic_screen = fetch_screen_text(tool_api, instance_id, ScreenSource::Default).ok();
    bail!(
        "step {} semantic wait timed out on instance {} ({}) last_snapshot={:?} diagnostic_screen={:?}",
        step.id,
        instance_id,
        semantic_wait_description(step),
        Some(last_snapshot),
        diagnostic_screen
    )
}

fn is_browser_ui_snapshot_transient(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("wait_for_ui_state timed out")
        || message.contains("request:ui_state timed out")
        || message.contains("Playwright driver ui_state timed out")
        || message.contains("Target page, context or browser has been closed")
        || message.contains("ui_state timed out for request")
}

fn wait_for_parity(
    step: &ScenarioStep,
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
            if let Some((snapshot, version)) =
                tool_api.wait_for_ui_snapshot_event(instance_id, remaining, local_version)?
            {
                local = snapshot;
                local_version = Some(version);
            } else {
                std::thread::sleep(Duration::from_millis(40));
                local = fetch_ui_snapshot(tool_api, instance_id)?;
            }
        } else if !wait_local_next && peer_backend_kind == "playwright_browser" {
            if let Some((snapshot, version)) =
                tool_api.wait_for_ui_snapshot_event(peer_instance, remaining, peer_version)?
            {
                peer = snapshot;
                peer_version = Some(version);
            } else {
                std::thread::sleep(Duration::from_millis(40));
                peer = fetch_ui_snapshot(tool_api, peer_instance)?;
            }
        } else {
            std::thread::sleep(Duration::from_millis(40));
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
    mode: FallbackObservationMode,
) -> Result<bool> {
    debug_assert_eq!(mode, FallbackObservationMode::DiagnosticOnly);
    let payload = dispatch_payload(
        tool_api,
        ToolRequest::Screen {
            instance_id: instance_id.to_string(),
            screen_source: ScreenSource::Default,
        },
    )?;
    let screen = payload
        .get("authoritative_screen")
        .and_then(serde_json::Value::as_str)
        .or_else(|| payload.get("screen").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    Ok(screen.contains(needle))
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
                screen_source: step.screen_source.unwrap_or_default(),
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
                .is_some_and(|name| channel_name_matches(name, channel))
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
            .position(|(name, _)| channel_name_matches(name, &target));
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

fn channel_name_matches(candidate: &str, target: &str) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .trim_start_matches('#')
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };

    let candidate = normalize(candidate);
    let target = normalize(target);
    if candidate.is_empty() || target.is_empty() {
        return false;
    }
    candidate == target || candidate.contains(&target) || target.contains(&candidate)
}

fn allow_missing_help_toast(expected_contains: &str, last_chat_command: Option<&str>) -> bool {
    let expected = expected_contains.trim().to_ascii_lowercase();
    let Some(command) = last_chat_command.map(str::trim) else {
        return false;
    };
    let help_expected =
        expected.contains("use ? for tui help") || expected.contains("/kick <user> [reason]");
    if help_expected {
        return command == "help"
            || command == "h"
            || command == "?"
            || command.starts_with("help ")
            || command.starts_with("h ")
            || command.starts_with("? ");
    }
    let whois_expected = expected.contains("user:");
    if whois_expected {
        return command.starts_with("whois ");
    }
    false
}

fn allow_missing_command_result_toast(
    last_chat_command: Option<&str>,
    expected_status: Option<ExpectedCommandStatus>,
    expected_reason_code: Option<&str>,
    expected_consistency: Option<ExpectedConsistency>,
) -> bool {
    let Some(command) = last_chat_command.map(str::trim) else {
        return false;
    };
    if command.starts_with("nhadd ")
        && matches!(expected_status, Some(ExpectedCommandStatus::Ok))
        && expected_reason_code.map_or(true, |value| value.eq_ignore_ascii_case("none"))
    {
        return matches!(
            expected_consistency,
            None | Some(ExpectedConsistency::Accepted)
        );
    }
    false
}

fn allow_missing_denied_toast(
    last_chat_command: Option<&str>,
    reason: Option<DeniedReason>,
    expected_status: Option<ExpectedCommandStatus>,
    expected_reason_code: Option<&str>,
) -> bool {
    let Some(command) = last_chat_command.map(str::trim) else {
        return false;
    };
    if command.starts_with("nhlink ")
        && reason.map_or(true, |value| value == DeniedReason::Permission)
        && expected_status.map_or(true, |value| value == ExpectedCommandStatus::Denied)
        && expected_reason_code.map_or(true, |value| {
            value.eq_ignore_ascii_case("permission_denied")
        })
    {
        return true;
    }
    false
}

fn command_result_contains_matches(expected_contains: &str, message: &str) -> bool {
    if message.contains(expected_contains) {
        return true;
    }

    let expected = expected_contains.trim().to_ascii_lowercase();
    let message = message.to_ascii_lowercase();
    match expected.as_str() {
        // Browser flow can emit explicit join text instead of "membership updated".
        "membership updated" => message.contains("joined "),
        // Browser flow can emit explicit invite text instead of "invitation sent".
        "invitation sent" => message.contains("invited "),
        _ => false,
    }
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

fn dispatch_clipboard_text(tool_api: &mut ToolApi, instance_id: &str, text: &str) -> Result<()> {
    if text.chars().count() <= CLIPBOARD_PASTE_CHUNK_CHARS {
        return dispatch(
            tool_api,
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
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: instance_id.to_string(),
                    keys: chunk.clone(),
                },
            )?;
            chunk.clear();
            chunk_len = 0;
            std::thread::sleep(Duration::from_millis(CLIPBOARD_PASTE_INTER_CHUNK_DELAY_MS));
        }
    }

    if !chunk.is_empty() {
        dispatch(
            tool_api,
            ToolRequest::SendKeys {
                instance_id: instance_id.to_string(),
                keys: chunk,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        InstanceConfig, InstanceMode, RunConfig, RunSection, ScenarioAction,
        ScenarioCanonicalModel,
    };
    use crate::coordinator::HarnessCoordinator;
    use aura_app::ui::contract::{
        ConfirmationState, ListId, ListItemSnapshot, ListSnapshot, OperationId,
        OperationInstanceId, OperationSnapshot, OperationState, ScreenId, SelectionSnapshot,
        UiReadiness, UiSnapshot,
    };
    use aura_app::ui_contract::{next_projection_revision, QuiescenceSnapshot};

    fn run_report_once(run: &RunConfig, scenario: &ScenarioConfig) -> ScenarioReport {
        let mut tool_api = ToolApi::new(
            HarnessCoordinator::from_run_config(run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = tool_api.start_all() {
            panic!("start_all failed: {error}");
        }
        let report = ScenarioExecutor::new(ExecutionMode::Scripted)
            .execute(scenario, &mut tool_api)
            .unwrap_or_else(|error| panic!("execute failed: {error}"));
        if let Err(error) = tool_api.stop_all() {
            panic!("stop_all failed: {error}");
        }
        report
    }

    fn test_scenario_config(id: &str, goal: &str, steps: Vec<ScenarioStep>) -> ScenarioConfig {
        ScenarioConfig {
            schema_version: 1,
            id: id.to_string(),
            goal: goal.to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps,
            canonical_model: ScenarioCanonicalModel::CompatibilityStepBridge,
            canonical_semantic_steps: Vec::new(),
        }
    }

    #[test]
    fn expected_consistency_accepts_stronger_observed_levels() {
        assert!(ExpectedConsistency::Accepted.is_satisfied_by(ExpectedConsistency::Accepted));
        assert!(ExpectedConsistency::Accepted.is_satisfied_by(ExpectedConsistency::Replicated));
        assert!(ExpectedConsistency::Accepted.is_satisfied_by(ExpectedConsistency::Enforced));
        assert!(ExpectedConsistency::Replicated.is_satisfied_by(ExpectedConsistency::Replicated));
        assert!(ExpectedConsistency::Replicated.is_satisfied_by(ExpectedConsistency::Enforced));
        assert!(ExpectedConsistency::Enforced.is_satisfied_by(ExpectedConsistency::Enforced));
    }

    #[test]
    fn expected_consistency_rejects_weaker_or_unrelated_levels() {
        assert!(!ExpectedConsistency::Replicated.is_satisfied_by(ExpectedConsistency::Accepted));
        assert!(!ExpectedConsistency::Enforced.is_satisfied_by(ExpectedConsistency::Accepted));
        assert!(!ExpectedConsistency::Enforced.is_satisfied_by(ExpectedConsistency::Replicated));
        assert!(!ExpectedConsistency::Accepted.is_satisfied_by(ExpectedConsistency::PartialTimeout));
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
                    }
                    .kind(),
                    contract: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
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
                    }
                    .kind(),
                    contract: IntentAction::SendChatMessage {
                        message: "hello".to_string(),
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
                .any(|failure| failure.contains("quiescence=")),
            "expected quiescence failure, got {failures:?}"
        );
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("runtime_event=MessageDeliveryReady")),
            "expected runtime-event failure, got {failures:?}"
        );
    }

    #[test]
    fn help_toast_fallback_is_scoped_to_help_expectations() {
        assert!(allow_missing_help_toast("Use ? for TUI help", Some("help")));
        assert!(allow_missing_help_toast(
            "/kick <user> [reason]",
            Some("help kick")
        ));
        assert!(allow_missing_help_toast("Use ? for TUI help", Some("?")));
        assert!(allow_missing_help_toast(
            "User:",
            Some("whois authority-abc")
        ));
        assert!(!allow_missing_help_toast(
            "Use ? for TUI help",
            Some("join slash-lab")
        ));
        assert!(!allow_missing_help_toast("status=ok", Some("help")));
    }

    #[test]
    fn nhadd_command_result_fallback_is_narrowly_scoped() {
        assert!(allow_missing_command_result_toast(
            Some("nhadd home-1"),
            Some(ExpectedCommandStatus::Ok),
            Some("none"),
            Some(ExpectedConsistency::Accepted),
        ));
        assert!(!allow_missing_command_result_toast(
            Some("nhadd home-1"),
            Some(ExpectedCommandStatus::Denied),
            Some("none"),
            Some(ExpectedConsistency::Accepted),
        ));
        assert!(!allow_missing_command_result_toast(
            Some("join slash-lab"),
            Some(ExpectedCommandStatus::Ok),
            Some("none"),
            Some(ExpectedConsistency::Accepted),
        ));
    }

    #[test]
    fn nhlink_denied_fallback_is_narrowly_scoped() {
        assert!(allow_missing_denied_toast(
            Some("nhlink home-1"),
            Some(DeniedReason::Permission),
            Some(ExpectedCommandStatus::Denied),
            Some("permission_denied"),
        ));
        assert!(!allow_missing_denied_toast(
            Some("kick bob"),
            Some(DeniedReason::Permission),
            Some(ExpectedCommandStatus::Denied),
            Some("permission_denied"),
        ));
    }

    #[test]
    fn semantic_wait_can_require_confirmed_list_items() {
        let step = crate::config::ScenarioStep {
            id: "wait-confirmed-contact".to_string(),
            action: crate::config::ScenarioAction::WaitFor,
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
    fn semantic_wait_rejects_pending_local_when_confirmed_is_required() {
        let step = crate::config::ScenarioStep {
            id: "wait-confirmed-contact".to_string(),
            action: crate::config::ScenarioAction::WaitFor,
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
        let step = crate::config::ScenarioStep {
            id: "wait-ready".to_string(),
            action: crate::config::ScenarioAction::WaitFor,
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
        let step = crate::config::ScenarioStep {
            id: "wait-op".to_string(),
            action: crate::config::ScenarioAction::WaitFor,
            operation_id: Some(OperationId::invitation_accept()),
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
                id: OperationId::invitation_accept(),
                instance_id: OperationInstanceId("test-operation-instance".to_string()),
                state: OperationState::Succeeded,
            }],
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(semantic_wait_matches(&step, &snapshot));
    }

    #[test]
    fn command_result_contains_aliases_membership_updates() {
        assert!(command_result_contains_matches(
            "membership updated",
            "joined #modal-lab status=ok reason=none consistency=replicated"
        ));
        assert!(!command_result_contains_matches(
            "membership updated",
            "invited authority-abc status=ok reason=none consistency=enforced"
        ));
    }

    #[test]
    fn command_result_contains_aliases_invitation_sent() {
        assert!(command_result_contains_matches(
            "invitation sent",
            "invited authority-abc status=ok reason=none consistency=enforced"
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

        let mut scenario = test_scenario_config(
            "executor-smoke",
            "verify transitions",
            vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::Noop,
                instance: None,
                expect: None,
                timeout_ms: None,
                ..Default::default()
            }],
        );
        scenario.execution_mode = None;

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
    fn repeated_runs_with_same_seed_share_same_report_shape() {
        let temp_root = std::env::temp_dir().join("aura-harness-determinism-test");
        let _ = std::fs::create_dir_all(&temp_root);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-determinism".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
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
            vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::Noop,
                instance: None,
                expect: None,
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
            vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendChatCommand,
                instance: Some("alice".to_string()),
                expect: Some("join slash-lab".to_string()),
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
            ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api)
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
            if instance_id == "alice" && keys == "ihello-semantic\n"
        ));
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
            vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendClipboard,
                instance: Some("bob".to_string()),
                expect: Some("alice".to_string()),
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

    #[test]
    fn send_clipboard_long_payload_is_chunked_and_reassembled() {
        let temp_root = std::env::temp_dir().join("aura-harness-executor-send-clipboard-chunked");
        let _ = std::fs::create_dir_all(&temp_root);
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
                artifact_dir: None,
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
            vec![ScenarioStep {
                id: "step-1".to_string(),
                action: ScenarioAction::SendClipboard,
                instance: Some("bob".to_string()),
                expect: Some("alice".to_string()),
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

        let clipboard_path = alice_data.join(".harness-clipboard.txt");
        let _ = std::fs::write(&clipboard_path, format!("{long_payload}\n"));

        if let Err(error) =
            ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api)
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
            operation_id: OperationId::invitation_accept(),
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
        let step = crate::config::ScenarioStep {
            id: "declared-wait".to_string(),
            ..Default::default()
        };
        let start_device_contract = IntentAction::StartDeviceEnrollment {
            device_name: "phone".to_string(),
            code_name: "device_code".to_string(),
        }
        .contract();
        assert!(ensure_wait_contract_declared(
            &step,
            &start_device_contract,
            WaitContractRef::Modal(ModalId::AddDevice),
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
    fn missing_sync_prerequisites_fail_as_convergence_contract_violations() {
        let mut context = ScenarioContext::default();
        context.pending_convergence.insert(
            "alice".to_string(),
            vec![BarrierDeclaration::RuntimeEvent(
                RuntimeEventKind::PendingHomeInvitationReady,
            )],
        );
        let step = crate::config::ScenarioStep {
            id: "send-too-early".to_string(),
            instance: Some("alice".to_string()),
            action: crate::config::ScenarioAction::SendChatMessage,
            ..Default::default()
        };

        let error = ensure_post_operation_convergence_satisfied(&step, &context, "alice")
            .err()
            .unwrap_or_else(|| {
                panic!("missing convergence must fail before the next shared intent")
            });
        assert!(error.to_string().contains("convergence-contract violation"));
        assert!(
            error.to_string().contains("PendingHomeInvitationReady"),
            "error should surface the missing convergence requirement"
        );
    }

    #[test]
    fn semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks() {
        let source = include_str!("executor.rs");
        for helper in [
            "fn wait_for_semantic_state(",
            "fn wait_for_runtime_event(",
            "fn wait_for_operation_handle_state(",
        ] {
            let start = source
                .find(helper)
                .unwrap_or_else(|| panic!("missing helper source for {helper}"));
            let tail = &source[start..];
            let end = tail.find("\nfn ").unwrap_or(tail.len());
            let body = &tail[..end];
            assert!(
                !body.contains("wait_for_dom_patterns("),
                "{helper} must not resolve through DOM pattern fallbacks"
            );
            assert!(
                !body.contains("snapshot_dom("),
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
        assert!(source.contains("enum FallbackObservationMode"));
        assert!(source.contains("FallbackObservationMode::BoundedSecondary"));
        assert!(source.contains("FallbackObservationMode::DiagnosticOnly"));
        assert!(source.contains("fn diagnostic_screen_contains("));
        assert!(source.contains("debug_assert_eq!(mode, FallbackObservationMode::DiagnosticOnly)"));
    }
}

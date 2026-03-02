use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::config::{ScenarioConfig, ScenarioStep};
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
) -> Result<()> {
    match step.action.as_str() {
        "launch_instances" | "noop" => Ok(()),
        "send_keys" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            let keys = step.expect.clone().unwrap_or_else(|| "\n".to_string());
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: instance_id.to_string(),
                    keys,
                },
            )?;
            Ok(())
        }
        "send_chat_command" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            let command = step
                .expect
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("step {} missing expect command", step.id))?;
            let command = if command.starts_with('/') {
                command.to_string()
            } else {
                format!("/{command}")
            };

            // Clear any active toast/modal so command-result waits do not match stale UI.
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
                ToolRequest::SendKeys {
                    instance_id: instance_id.to_string(),
                    keys: format!("i{command}\n"),
                },
            )?;
            Ok(())
        }
        "send_clipboard" => {
            let target_instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing target instance", step.id))?;
            let source_instance_id = step
                .expect
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing source instance in expect", step.id))?;
            let timeout_ms = step.timeout_ms.unwrap_or(step_budget_ms);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let clipboard_text = loop {
                let attempt_error = match dispatch_payload(
                    tool_api,
                    ToolRequest::ReadClipboard {
                        instance_id: source_instance_id.to_string(),
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
                        "send_clipboard timed out for source={} target={} timeout_ms={} last_error={}",
                        source_instance_id,
                        target_instance_id,
                        timeout_ms,
                        attempt_error
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            dispatch(
                tool_api,
                ToolRequest::SendKeys {
                    instance_id: target_instance_id.to_string(),
                    keys: clipboard_text,
                },
            )?;
            Ok(())
        }
        "send_key" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            let key_name = step
                .expect
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing expect key", step.id))?;
            let key = parse_tool_key(key_name)?;
            dispatch(
                tool_api,
                ToolRequest::SendKey {
                    instance_id: instance_id.to_string(),
                    key,
                    repeat: 1,
                },
            )?;
            Ok(())
        }
        "wait_for" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            let pattern = step
                .expect
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing expect pattern", step.id))?;
            dispatch(
                tool_api,
                ToolRequest::WaitFor {
                    instance_id: instance_id.to_string(),
                    pattern: pattern.to_string(),
                    timeout_ms: step.timeout_ms.unwrap_or(step_budget_ms),
                },
            )?;
            Ok(())
        }
        "restart" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            dispatch(
                tool_api,
                ToolRequest::Restart {
                    instance_id: instance_id.to_string(),
                },
            )?;
            Ok(())
        }
        "kill" => {
            let instance_id = step
                .instance
                .as_deref()
                .ok_or_else(|| anyhow!("step {} missing instance", step.id))?;
            dispatch(
                tool_api,
                ToolRequest::Kill {
                    instance_id: instance_id.to_string(),
                },
            )?;
            Ok(())
        }
        "fault_delay" => {
            let delay_ms = step
                .timeout_ms
                .unwrap_or_else(|| 25 + fault_rng.range_u64(0, 25));
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            Ok(())
        }
        "fault_loss" | "fault_tunnel_drop" => {
            // Consume deterministic RNG state so replay and injected faults are seed-driven.
            let _decision = scenario_rng.range_u64(0, 2);
            Ok(())
        }
        action => bail!("unsupported scenario action: {action}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection};
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
                action: "noop".to_string(),
                instance: None,
                expect: None,
                timeout_ms: None,
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
    fn state_machine_rejects_unknown_action() {
        let temp_root = std::env::temp_dir().join("aura-harness-executor-test-2");
        let _ = std::fs::create_dir_all(&temp_root);

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "executor-test-2".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(6),
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
                bind_address: "127.0.0.1:45002".to_string(),
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
            id: "executor-invalid".to_string(),
            goal: "verify action validation".to_string(),
            execution_mode: Some("scripted".to_string()),
            required_capabilities: vec![],
            steps: vec![ScenarioStep {
                id: "step-1".to_string(),
                action: "unsupported_action".to_string(),
                instance: None,
                expect: None,
                timeout_ms: None,
            }],
        };

        let mut api = ToolApi::new(
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
        );
        if let Err(error) = api.start_all() {
            panic!("start_all failed: {error}");
        }

        let error =
            match ScenarioExecutor::new(ExecutionMode::Scripted).execute(&scenario, &mut api) {
                Ok(_) => panic!("unsupported actions must fail"),
                Err(error) => error,
            };
        if let Err(error) = api.stop_all() {
            panic!("stop_all failed: {error}");
        }

        assert!(error.to_string().contains("unsupported scenario action"));
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
                action: "send_chat_command".to_string(),
                instance: Some("alice".to_string()),
                expect: Some("join slash-lab".to_string()),
                timeout_ms: None,
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
        assert!(action_log.len() >= 2, "expected at least two tool actions");

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
                assert_eq!(keys, "i/join slash-lab\n");
            }
            other => panic!("expected SendKeys second, got {other:?}"),
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
                action: "send_clipboard".to_string(),
                instance: Some("bob".to_string()),
                expect: Some("alice".to_string()),
                timeout_ms: Some(2000),
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

//! # ITF Trace Replay for TUI State Machine
//!
//! Replays ITF traces from Quint against the TUI state machine for
//! model-based testing and verification.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_terminal::testing::itf_replay::{ITFTraceReplayer, TuiITFState};
//!
//! let replayer = ITFTraceReplayer::new();
//! let results = replayer.replay_trace_file("verification/traces/tui_trace.itf.json")?;
//! assert!(results.all_states_match);
//! ```

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use aura_core::effects::terminal::{events, KeyCode, KeyEvent, TerminalEvent};

use crate::tui::screens::Router;
use crate::tui::screens::Screen;
use crate::tui::state_machine::{ModalType, TuiCommand, TuiState};

/// ITF trace structure matching Quint output
#[derive(Debug, Clone, Deserialize)]
pub struct ITFTrace {
    #[serde(rename = "#meta")]
    pub meta: ITFMeta,
    pub vars: Vec<String>,
    pub states: Vec<ITFState>,
}

/// ITF trace metadata
#[derive(Debug, Clone, Deserialize)]
pub struct ITFMeta {
    pub format: String,
    pub source: String,
    pub status: String,
}

/// Single state in ITF trace
#[derive(Debug, Clone, Deserialize)]
pub struct ITFState {
    #[serde(rename = "#meta")]
    pub meta: ITFStateMeta,
    #[serde(flatten)]
    pub variables: HashMap<String, serde_json::Value>,
}

/// State metadata
#[derive(Debug, Clone, Deserialize)]
pub struct ITFStateMeta {
    pub index: usize,
}

/// TUI state extracted from ITF format
#[derive(Debug, Clone, PartialEq)]
pub struct TuiITFState {
    pub current_screen: Screen,
    pub current_modal: ModalType,
    pub neighborhood_insert_mode: bool,
    pub chat_insert_mode: bool,
    pub should_exit: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub command_queue: Vec<ItfCommand>,
}

/// Commands in the ITF trace (Quint model output).
///
/// The Rust state machine has many more commands; for ITF replay we compare only
/// the subset modeled by the Quint spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItfCommand {
    Exit,
    ShowToast,
    DismissToast,
    Render,
}

/// Result of replaying a single step
#[derive(Debug)]
pub struct StepResult {
    pub step_index: usize,
    pub expected: TuiITFState,
    pub actual: TuiState,
    pub matches: bool,
    pub diff: Option<String>,
}

/// Result of replaying an entire trace
#[derive(Debug)]
pub struct ReplayResult {
    pub total_steps: usize,
    pub matched_steps: usize,
    pub failed_steps: Vec<StepResult>,
    pub all_states_match: bool,
}

/// Replays ITF traces against TUI state machine
pub struct ITFTraceReplayer;

impl ITFTraceReplayer {
    /// Create a new replayer
    pub fn new() -> Self {
        Self
    }

    /// Replay a trace from file
    ///
    /// Note: Uses std::fs directly as this is test infrastructure reading external
    /// ITF trace files (Quint model output). Test tooling is exempt from StorageEffects
    /// requirements per Layer 8 guidelines.
    pub fn replay_trace_file(&self, path: impl AsRef<Path>) -> Result<ReplayResult, String> {
        #[allow(clippy::disallowed_methods)] // Test infrastructure reading external trace files
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read ITF file: {}", e))?;
        let trace: ITFTrace = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse ITF JSON: {}", e))?;
        self.replay_trace(&trace)
    }

    /// Replay a parsed trace
    pub fn replay_trace(&self, trace: &ITFTrace) -> Result<ReplayResult, String> {
        let mut failed_steps = Vec::new();
        let total_steps = trace.states.len();

        let mut expected_states = Vec::with_capacity(total_steps);
        for itf_state in &trace.states {
            expected_states.push(Self::extract_tui_state(&itf_state.variables)?);
        }

        // Validate invariants and step-to-step reachability.
        for i in 0..expected_states.len() {
            let expected = &expected_states[i];

            if !Self::validate_state_invariants(expected) {
                failed_steps.push(StepResult {
                    step_index: i,
                    expected: expected.clone(),
                    actual: self.create_matching_tui_state(expected),
                    matches: false,
                    diff: Some("State violates invariants".to_string()),
                });
                continue;
            }

            // For the first state, we only validate invariants.
            if i == 0 {
                continue;
            }

            let from = self.create_matching_tui_state(&expected_states[i - 1]);
            let to = expected;

            if self.find_transition_witness(&from, to).is_none() {
                failed_steps.push(StepResult {
                    step_index: i,
                    expected: expected.clone(),
                    actual: self.create_matching_tui_state(expected),
                    matches: false,
                    diff: Some(
                        "No modeled event produces expected next state from previous state"
                            .to_string(),
                    ),
                });
            }
        }

        let matched_steps = total_steps - failed_steps.len();
        let all_states_match = failed_steps.is_empty();
        Ok(ReplayResult {
            total_steps,
            matched_steps,
            failed_steps,
            all_states_match,
        })
    }

    /// Extract TUI state from ITF variables
    fn extract_tui_state(vars: &HashMap<String, serde_json::Value>) -> Result<TuiITFState, String> {
        let current_screen =
            Self::parse_screen(vars.get("currentScreen").ok_or("Missing currentScreen")?)?;

        let current_modal =
            Self::parse_modal(vars.get("currentModal").ok_or("Missing currentModal")?)?;

        let neighborhood_insert_mode = vars
            .get("blockInsertMode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let chat_insert_mode = vars
            .get("chatInsertMode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let should_exit = vars
            .get("shouldExit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let terminal_width =
            Self::parse_bigint(vars.get("terminalWidth").ok_or("Missing terminalWidth")?)? as u16;

        let terminal_height =
            Self::parse_bigint(vars.get("terminalHeight").ok_or("Missing terminalHeight")?)? as u16;

        let command_queue = match vars.get("commandQueue") {
            Some(v) => Self::parse_command_queue(v)?,
            None => Vec::new(),
        };

        Ok(TuiITFState {
            current_screen,
            current_modal,
            neighborhood_insert_mode,
            chat_insert_mode,
            should_exit,
            terminal_width,
            terminal_height,
            command_queue,
        })
    }

    /// Parse screen from ITF tagged value
    fn parse_screen(value: &serde_json::Value) -> Result<Screen, String> {
        let tag = value
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or("Screen missing tag")?;

        match tag {
            "Block" => Ok(Screen::Neighborhood),
            "Chat" => Ok(Screen::Chat),
            "Contacts" => Ok(Screen::Contacts),
            "Invitations" => Ok(Screen::Contacts),
            "Neighborhood" => Ok(Screen::Neighborhood),
            "Notifications" => Ok(Screen::Notifications),
            "Settings" => Ok(Screen::Settings),
            "Recovery" => Ok(Screen::Notifications),
            // "Invitations" was removed - functionality moved to Contacts screen
            _ => Err(format!("Unknown screen: {}", tag)),
        }
    }

    /// Parse modal from ITF tagged value
    fn parse_modal(value: &serde_json::Value) -> Result<ModalType, String> {
        let tag = value
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or("Modal missing tag")?;

        match tag {
            "NoModal" => Ok(ModalType::None),
            "HelpModal" => Ok(ModalType::Help),
            "AccountSetupModal" => Ok(ModalType::AccountSetup),
            "GuardianSelectModal" => Ok(ModalType::GuardianSelect),
            "ContactSelectModal" => Ok(ModalType::ContactSelect),
            "ConfirmModal" => Ok(ModalType::Confirm),
            // Screen-specific modals - these are now tracked via screen-specific state
            // (e.g., state.chat.topic_modal.visible) rather than ModalType variants.
            // Map to None for ITF compatibility; actual modal state is in screen fields.
            "CreateChannelModal"
            | "ChannelInfoModal"
            | "SetTopicModal"
            | "CreateInvitationModal"
            | "ImportInvitationModal"
            | "ExportInvitationModal"
            | "TextInputModal"
            | "ThresholdConfigModal"
            | "InvitationCodeModal" => Ok(ModalType::None),
            _ => Err(format!("Unknown modal: {}", tag)),
        }
    }

    fn parse_command_queue(value: &serde_json::Value) -> Result<Vec<ItfCommand>, String> {
        let Some(items) = value.as_array() else {
            return Err("commandQueue is not an array".to_string());
        };

        let mut commands = Vec::with_capacity(items.len());
        for cmd in items {
            let tag = cmd
                .get("tag")
                .and_then(|v| v.as_str())
                .ok_or("Command missing tag")?;
            let mapped = match tag {
                "ExitCommand" => ItfCommand::Exit,
                "ShowToastCommand" => ItfCommand::ShowToast,
                "DismissToastCommand" => ItfCommand::DismissToast,
                "RenderCommand" => ItfCommand::Render,
                other => return Err(format!("Unknown command: {other}")),
            };
            commands.push(mapped);
        }

        Ok(commands)
    }

    /// Parse bigint from ITF format
    fn parse_bigint(value: &serde_json::Value) -> Result<i64, String> {
        // ITF encodes bigints as {"#bigint": "value"}
        if let Some(obj) = value.as_object() {
            if let Some(bigint) = obj.get("#bigint") {
                return bigint
                    .as_str()
                    .ok_or("Bigint not a string")?
                    .parse()
                    .map_err(|e| format!("Invalid bigint: {}", e));
            }
        }
        // Fall back to regular number
        value.as_i64().ok_or("Not a valid integer".to_string())
    }

    /// Validate state invariants (mirrors Quint spec)
    fn validate_state_invariants(state: &TuiITFState) -> bool {
        // Insert mode only valid on Neighborhood or Chat screens
        let insert_mode_valid = (!state.neighborhood_insert_mode && !state.chat_insert_mode)
            || (state.current_screen == Screen::Neighborhood && state.neighborhood_insert_mode)
            || (state.current_screen == Screen::Chat && state.chat_insert_mode);

        // Terminal size is reasonable
        let size_valid = state.terminal_width >= 10
            && state.terminal_width <= 500
            && state.terminal_height >= 5
            && state.terminal_height <= 200;

        insert_mode_valid && size_valid
    }

    fn project_tui_state(state: &TuiState, commands: &[TuiCommand]) -> TuiITFState {
        TuiITFState {
            current_screen: state.screen(),
            current_modal: state.current_modal_type(),
            neighborhood_insert_mode: state.neighborhood.insert_mode,
            chat_insert_mode: state.chat.insert_mode,
            should_exit: state.should_exit,
            terminal_width: state.terminal_size.0,
            terminal_height: state.terminal_size.1,
            command_queue: commands
                .iter()
                .filter_map(|c| match c {
                    TuiCommand::Exit => Some(ItfCommand::Exit),
                    TuiCommand::ShowToast { .. } => Some(ItfCommand::ShowToast),
                    TuiCommand::DismissToast { .. } => Some(ItfCommand::DismissToast),
                    TuiCommand::Render => Some(ItfCommand::Render),
                    _ => None,
                })
                .collect(),
        }
    }

    fn find_transition_witness(
        &self,
        from: &TuiState,
        expected_to: &TuiITFState,
    ) -> Option<(TerminalEvent, TuiState, Vec<TuiCommand>)> {
        let mut candidates: Vec<TerminalEvent> = Vec::new();

        // Always allow a tick as a "no-op" witness when toasts are absent.
        candidates.push(events::tick());

        // Keys modeled by the Quint spec.
        candidates.push(events::escape());
        candidates.push(events::char('q'));
        candidates.push(events::char('?'));
        candidates.push(events::char('i'));

        for c in ['1', '2', '3', '4', '5'] {
            candidates.push(events::char(c));
        }

        candidates.push(events::tab());
        candidates.push(TerminalEvent::Key(KeyEvent::press(KeyCode::BackTab)));

        // If this step is a resize in the spec, the next state encodes the desired dimensions.
        candidates.push(events::resize(
            expected_to.terminal_width,
            expected_to.terminal_height,
        ));

        for event in candidates {
            let (state, cmds) = crate::tui::state_machine::transition(from, event.clone());
            let projected = Self::project_tui_state(&state, &cmds);
            if &projected == expected_to {
                return Some((event, state, cmds));
            }
        }

        None
    }

    /// Create a TuiState matching the ITF state (for comparison)
    fn create_matching_tui_state(&self, itf: &TuiITFState) -> TuiState {
        use crate::tui::state_machine::{AccountSetupModalState, ModalQueue, QueuedModal};

        // Convert ModalType to QueuedModal for queue-based system
        let mut modal_queue = ModalQueue::new();
        match itf.current_modal {
            ModalType::AccountSetup => {
                modal_queue.enqueue(QueuedModal::AccountSetup(AccountSetupModalState::default()));
            }
            ModalType::Help => {
                modal_queue.enqueue(QueuedModal::Help {
                    current_screen: Some(itf.current_screen),
                });
            }
            ModalType::GuardianSelect => {
                modal_queue.enqueue(QueuedModal::GuardianSelect(
                    crate::tui::state_machine::ContactSelectModalState::single(
                        "Select Guardian",
                        Vec::new(),
                    ),
                ));
            }
            ModalType::ContactSelect => {
                modal_queue.enqueue(QueuedModal::ContactSelect(
                    crate::tui::state_machine::ContactSelectModalState::single(
                        "Select Contact",
                        Vec::new(),
                    ),
                ));
            }
            ModalType::Confirm => {
                modal_queue.enqueue(QueuedModal::Confirm {
                    title: "Confirm".to_string(),
                    message: "Are you sure?".to_string(),
                    on_confirm: None,
                });
            }
            ModalType::None => {
                // No modal - leave queue empty
            }
        }

        TuiState {
            router: Router::new(itf.current_screen),
            modal_queue,
            neighborhood: crate::tui::state_machine::NeighborhoodViewState {
                insert_mode: itf.neighborhood_insert_mode,
                ..Default::default()
            },
            chat: crate::tui::state_machine::ChatViewState {
                insert_mode: itf.chat_insert_mode,
                ..Default::default()
            },
            should_exit: itf.should_exit,
            terminal_size: (itf.terminal_width, itf.terminal_height),
            ..Default::default()
        }
    }
}

impl Default for ITFTraceReplayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_screen() {
        let value = serde_json::json!({"tag": "Block", "value": {"#tup": []}});
        assert_eq!(
            ITFTraceReplayer::parse_screen(&value).unwrap(),
            Screen::Neighborhood
        );

        let value = serde_json::json!({"tag": "Chat", "value": {"#tup": []}});
        assert_eq!(
            ITFTraceReplayer::parse_screen(&value).unwrap(),
            Screen::Chat
        );
    }

    #[test]
    fn test_parse_modal() {
        let value = serde_json::json!({"tag": "NoModal", "value": {"#tup": []}});
        assert_eq!(
            ITFTraceReplayer::parse_modal(&value).unwrap(),
            ModalType::None
        );

        let value = serde_json::json!({"tag": "HelpModal", "value": {"#tup": []}});
        assert_eq!(
            ITFTraceReplayer::parse_modal(&value).unwrap(),
            ModalType::Help
        );
    }

    #[test]
    fn test_parse_bigint() {
        let value = serde_json::json!({"#bigint": "80"});
        assert_eq!(ITFTraceReplayer::parse_bigint(&value).unwrap(), 80);

        let value = serde_json::json!(42);
        assert_eq!(ITFTraceReplayer::parse_bigint(&value).unwrap(), 42);
    }

    #[test]
    fn test_validate_invariants() {
        // Valid state
        let state = TuiITFState {
            current_screen: Screen::Neighborhood,
            current_modal: ModalType::None,
            neighborhood_insert_mode: true,
            chat_insert_mode: false,
            should_exit: false,
            terminal_width: 80,
            terminal_height: 24,
            command_queue: Vec::new(),
        };
        assert!(ITFTraceReplayer::validate_state_invariants(&state));

        // Invalid: insert mode on wrong screen
        let state = TuiITFState {
            current_screen: Screen::Contacts,
            current_modal: ModalType::None,
            neighborhood_insert_mode: true, // Invalid: not on Neighborhood screen
            chat_insert_mode: false,
            should_exit: false,
            terminal_width: 80,
            terminal_height: 24,
            command_queue: Vec::new(),
        };
        assert!(!ITFTraceReplayer::validate_state_invariants(&state));
    }
}

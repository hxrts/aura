//! Scenario export functionality
//!
//! Converts interactive command sequences from branches into reproducible TOML
//! scenario files that can be loaded and re-run deterministically.

use anyhow::Result;
use aura_console_types::{ConsoleCommand, EventType, TraceEvent};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::branch_manager::{BranchMetadata, SimulationBranch};
use crate::simulation_wrapper::SimulationWrapper;

/// Represents a scenario action derived from an interactive command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ScenarioAction {
    /// Step simulation forward
    Step { count: u64, at_tick: u64 },
    /// Step until condition is met
    StepUntil {
        condition: String,
        at_tick: u64,
        max_steps: Option<u64>,
    },
    /// Initiate DKD protocol
    InitiateDkd {
        participants: Vec<String>,
        app_id: String,
        context: String,
        at_tick: u64,
    },
    /// Initiate recovery protocol
    InitiateRecovery {
        participants: Vec<String>,
        recovery_data: String,
        at_tick: u64,
    },
    /// Create network partition
    Partition {
        participants: Vec<String>,
        at_tick: u64,
    },
    /// Heal network partitions
    Heal { at_tick: u64 },
    /// Add network delay
    Delay {
        from: String,
        to: String,
        delay_ms: u64,
        at_tick: u64,
    },
    /// Make participant byzantine
    Byzantine {
        participant: String,
        strategy: String,
        at_tick: u64,
    },
    /// Inject custom event
    Inject {
        participant: String,
        event_type: String,
        at_tick: u64,
    },
}

/// Scenario setup configuration derived from branch state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSetup {
    /// Number of participants
    pub participant_count: usize,
    /// Threshold for threshold protocols
    pub threshold: usize,
    /// Protocol type (default for most scenarios)
    pub protocol: String,
    /// Network conditions
    pub network_conditions: Option<NetworkConditions>,
    /// Byzantine conditions
    pub byzantine_conditions: Option<ByzantineConditions>,
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Maximum ticks
    pub max_ticks: Option<u64>,
}

/// Network conditions configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConditions {
    /// Base latency in milliseconds
    pub latency_ms: Option<u64>,
    /// Message drop rate (0.0 to 1.0)
    pub drop_rate: Option<f64>,
    /// Network partitions
    pub partitions: Option<Vec<Vec<String>>>,
}

/// Byzantine conditions configuration  
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineConditions {
    /// Number of byzantine participants
    pub count: usize,
    /// Byzantine strategies
    pub strategies: Vec<String>,
}

/// Complete exported scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedScenario {
    /// Scenario name
    pub name: String,
    /// Scenario description
    pub description: Option<String>,
    /// Setup configuration
    pub setup: ScenarioSetup,
    /// Sequence of actions to execute
    pub actions: Vec<ScenarioAction>,
    /// Assertions to check (currently empty)
    pub assertions: Vec<ScenarioAssertion>,
    /// Metadata about the export
    pub metadata: ExportMetadata,
}

/// Assertion to check during scenario execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAssertion {
    /// Assertion type
    pub assertion_type: String,
    /// When to check (tick number)
    pub at_tick: u64,
    /// Expected value or condition
    pub expected: String,
}

/// Metadata about the scenario export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMetadata {
    /// Original branch ID
    pub source_branch_id: String,
    /// Export timestamp
    pub exported_at: SystemTime,
    /// Parent branch ID (if forked)
    pub parent_branch_id: Option<String>,
    /// Fork point tick (if forked)
    pub fork_tick: Option<u64>,
    /// Random seed used
    pub seed: u64,
    /// Tool version info
    pub exported_by: String,
}

/// Command execution record for tracking during branch execution
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CommandRecord {
    /// The command that was executed
    pub command: ConsoleCommand,
    /// Tick when command was executed
    pub executed_at_tick: u64,
    /// Execution timestamp
    pub executed_at: SystemTime,
    /// Command sequence number
    pub sequence: u64,
}

/// Scenario exporter that converts branch state to TOML scenarios
#[derive(Debug)]
pub struct ScenarioExporter {
    /// Command history tracking
    command_history: Vec<CommandRecord>,
    /// Sequence counter for commands
    next_sequence: u64,
}

#[allow(dead_code)]
impl ScenarioExporter {
    /// Create a new scenario exporter
    pub fn new() -> Self {
        Self {
            command_history: Vec::new(),
            next_sequence: 0,
        }
    }

    /// Record a command execution for later export
    pub fn record_command(&mut self, command: ConsoleCommand, executed_at_tick: u64) {
        let record = CommandRecord {
            command,
            executed_at_tick,
            executed_at: current_timestamp,
            sequence: self.next_sequence,
        };

        self.command_history.push(record);
        self.next_sequence += 1;
    }

    /// Export branch as a TOML scenario
    pub fn export_branch_as_scenario(
        &self,
        branch: &SimulationBranch,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<String> {
        let scenario = self.build_scenario(branch, name, description)?;
        self.serialize_to_toml(scenario)
    }

    /// Build scenario structure from branch state
    fn build_scenario(
        &self,
        branch: &SimulationBranch,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<ExportedScenario> {
        let simulation = branch.simulation.lock().unwrap();

        // Generate scenario name if not provided
        let scenario_name = name.unwrap_or_else(|| {
            branch
                .metadata
                .name
                .clone()
                .unwrap_or_else(|| format!("scenario_{}", branch.id))
        });

        // Build setup configuration
        let setup = self.build_setup_config(&simulation, &branch.event_buffer)?;

        // Convert command history to actions
        let actions = self.convert_commands_to_actions()?;

        // Build export metadata
        let metadata = ExportMetadata {
            source_branch_id: branch.id.to_string(),
            exported_at: current_timestamp,
            parent_branch_id: branch.metadata.parent_branch.map(|id| id.to_string()),
            fork_tick: self.determine_fork_tick(&branch.metadata),
            seed: simulation.seed,
            exported_by: "aura-sim-server v0.1.0".to_string(),
        };

        Ok(ExportedScenario {
            name: scenario_name,
            description,
            setup,
            actions,
            assertions: Vec::new(), // TODO: Extract assertions from events
            metadata,
        })
    }

    /// Build setup configuration from simulation state
    fn build_setup_config(
        &self,
        simulation: &SimulationWrapper,
        events: &[TraceEvent],
    ) -> Result<ScenarioSetup> {
        let participants = simulation.get_participants();
        let participant_count = participants.len();

        // Determine threshold (default to 2/3 majority)
        let threshold = if participant_count > 0 {
            (participant_count * 2 + 2) / 3
        } else {
            0
        };

        // Analyze events to determine network and byzantine conditions
        let (network_conditions, byzantine_conditions) =
            self.analyze_events_for_conditions(events)?;

        Ok(ScenarioSetup {
            participant_count,
            threshold,
            protocol: "Dkd".to_string(), // Default protocol
            network_conditions,
            byzantine_conditions,
            timeout_ms: Some(60000), // 60 seconds default
            max_ticks: Some(10000),  // 10k ticks default
        })
    }

    /// Analyze events to extract network and byzantine conditions
    fn analyze_events_for_conditions(
        &self,
        events: &[TraceEvent],
    ) -> Result<(Option<NetworkConditions>, Option<ByzantineConditions>)> {
        let mut partitions = Vec::new();
        let mut byzantine_participants = Vec::new();
        let mut has_delays = false;

        for event in events {
            match &event.event_type {
                EventType::EffectExecuted {
                    effect_type,
                    effect_data,
                } => match effect_type.as_str() {
                    "partition_created" => {
                        if let Ok(partition_str) = std::str::from_utf8(effect_data) {
                            let participants: Vec<String> =
                                partition_str.split(',').map(|s| s.to_string()).collect();
                            partitions.push(participants);
                        }
                    }
                    "byzantine_enabled" => {
                        if !byzantine_participants.contains(&event.participant) {
                            byzantine_participants.push(event.participant.clone());
                        }
                    }
                    "delay_added" => {
                        has_delays = true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let network_conditions = if !partitions.is_empty() || has_delays {
            Some(NetworkConditions {
                latency_ms: if has_delays { Some(50) } else { None },
                drop_rate: None,
                partitions: if !partitions.is_empty() {
                    Some(partitions)
                } else {
                    None
                },
            })
        } else {
            None
        };

        let byzantine_conditions = if !byzantine_participants.is_empty() {
            Some(ByzantineConditions {
                count: byzantine_participants.len(),
                strategies: vec!["DropMessages".to_string()], // Default strategy
            })
        } else {
            None
        };

        Ok((network_conditions, byzantine_conditions))
    }

    /// Convert command history to scenario actions
    fn convert_commands_to_actions(&self) -> Result<Vec<ScenarioAction>> {
        let mut actions = Vec::new();

        for record in &self.command_history {
            if let Some(action) =
                self.convert_command_to_action(&record.command, record.executed_at_tick)?
            {
                actions.push(action);
            }
        }

        Ok(actions)
    }

    /// Convert a single command to a scenario action
    fn convert_command_to_action(
        &self,
        command: &ConsoleCommand,
        at_tick: u64,
    ) -> Result<Option<ScenarioAction>> {
        let action = match command {
            ConsoleCommand::Step { count } => Some(ScenarioAction::Step {
                count: *count,
                at_tick,
            }),
            ConsoleCommand::InitiateDkd {
                participants,
                context,
            } => Some(ScenarioAction::InitiateDkd {
                participants: participants.clone(),
                app_id: String::new(), // No app_id in current command
                context: context.clone(),
                at_tick,
            }),
            ConsoleCommand::InitiateRecovery { guardians } => {
                Some(ScenarioAction::InitiateRecovery {
                    participants: guardians.clone(),
                    recovery_data: String::new(), // No recovery_data in current command
                    at_tick,
                })
            }
            ConsoleCommand::CreatePartition { devices } => Some(ScenarioAction::Partition {
                participants: devices.clone(),
                at_tick,
            }),
            ConsoleCommand::SetDeviceOffline { device_id } => Some(ScenarioAction::Byzantine {
                participant: device_id.clone(),
                strategy: "offline".to_string(),
                at_tick,
            }),
            ConsoleCommand::EnableByzantine {
                device_id,
                strategy,
            } => Some(ScenarioAction::Byzantine {
                participant: device_id.clone(),
                strategy: strategy.clone(),
                at_tick,
            }),
            ConsoleCommand::InjectMessage { to, message } => Some(ScenarioAction::Inject {
                participant: to.clone(),
                event_type: message.clone(),
                at_tick,
            }),
            // Read-only and control commands don't generate actions
            ConsoleCommand::InitiateResharing { .. }
            | ConsoleCommand::RunUntilIdle
            | ConsoleCommand::SeekToTick { .. }
            | ConsoleCommand::Checkpoint { .. }
            | ConsoleCommand::RestoreCheckpoint { .. }
            | ConsoleCommand::QueryState { .. }
            | ConsoleCommand::GetTopology
            | ConsoleCommand::GetLedger { .. }
            | ConsoleCommand::GetViolations
            | ConsoleCommand::ListBranches
            | ConsoleCommand::CheckoutBranch { .. }
            | ConsoleCommand::ForkBranch { .. }
            | ConsoleCommand::DeleteBranch { .. }
            | ConsoleCommand::ExportScenario { .. }
            | ConsoleCommand::LoadScenario { .. }
            | ConsoleCommand::LoadTrace { .. }
            | ConsoleCommand::GetCausalityPath { .. }
            | ConsoleCommand::GetEventsInRange { .. }
            | ConsoleCommand::BroadcastMessage { .. } => None,
        };

        Ok(action)
    }

    /// Determine fork tick from branch metadata
    fn determine_fork_tick(&self, metadata: &BranchMetadata) -> Option<u64> {
        // In a full implementation, this would track the exact tick when the branch was forked
        // For now, we'll use a heuristic based on the first command
        if metadata.parent_branch.is_some() && !self.command_history.is_empty() {
            Some(self.command_history[0].executed_at_tick)
        } else {
            None
        }
    }

    /// Serialize scenario to TOML format
    fn serialize_to_toml(&self, scenario: ExportedScenario) -> Result<String> {
        // Custom TOML serialization to match expected format
        let mut toml_content = String::new();

        // Header
        toml_content.push_str(&format!("name = \"{}\"\n", scenario.name));
        if let Some(desc) = &scenario.description {
            toml_content.push_str(&format!("description = \"{}\"\n", desc));
        }
        toml_content.push('\n');

        // Setup section
        toml_content.push_str("[setup]\n");
        toml_content.push_str(&format!(
            "participant_count = {}\n",
            scenario.setup.participant_count
        ));
        toml_content.push_str(&format!("threshold = {}\n", scenario.setup.threshold));
        toml_content.push_str(&format!("protocol = \"{}\"\n", scenario.setup.protocol));

        if let Some(timeout) = scenario.setup.timeout_ms {
            toml_content.push_str(&format!("timeout_ms = {}\n", timeout));
        }
        if let Some(max_ticks) = scenario.setup.max_ticks {
            toml_content.push_str(&format!("max_ticks = {}\n", max_ticks));
        }
        toml_content.push('\n');

        // Network conditions
        if let Some(network) = &scenario.setup.network_conditions {
            toml_content.push_str("[setup.network_conditions]\n");
            if let Some(latency) = network.latency_ms {
                toml_content.push_str(&format!("latency_ms = {}\n", latency));
            }
            if let Some(drop_rate) = network.drop_rate {
                toml_content.push_str(&format!("drop_rate = {}\n", drop_rate));
            }
            if let Some(partitions) = &network.partitions {
                toml_content.push_str("partitions = [\n");
                for partition in partitions {
                    toml_content.push_str("  [");
                    for (i, participant) in partition.iter().enumerate() {
                        if i > 0 {
                            toml_content.push_str(", ");
                        }
                        toml_content.push_str(&format!("\"{}\"", participant));
                    }
                    toml_content.push_str("],\n");
                }
                toml_content.push_str("]\n");
            }
            toml_content.push('\n');
        }

        // Byzantine conditions
        if let Some(byzantine) = &scenario.setup.byzantine_conditions {
            toml_content.push_str("[setup.byzantine_conditions]\n");
            toml_content.push_str(&format!("count = {}\n", byzantine.count));
            toml_content.push_str("strategies = [");
            for (i, strategy) in byzantine.strategies.iter().enumerate() {
                if i > 0 {
                    toml_content.push_str(", ");
                }
                toml_content.push_str(&format!("\"{}\"", strategy));
            }
            toml_content.push_str("]\n\n");
        }

        // Actions
        if !scenario.actions.is_empty() {
            for action in scenario.actions.iter() {
                toml_content.push_str(&format!("[[actions]]\n"));
                self.serialize_action_to_toml(&mut toml_content, action)?;
                toml_content.push('\n');
            }
        }

        // Metadata (as comments for reference)
        toml_content.push_str("# Export metadata:\n");
        toml_content.push_str(&format!(
            "# Source branch: {}\n",
            scenario.metadata.source_branch_id
        ));
        toml_content.push_str(&format!(
            "# Exported by: {}\n",
            scenario.metadata.exported_by
        ));
        toml_content.push_str(&format!("# Seed: {}\n", scenario.metadata.seed));
        if let Some(parent) = &scenario.metadata.parent_branch_id {
            toml_content.push_str(&format!("# Parent branch: {}\n", parent));
        }
        if let Some(fork_tick) = scenario.metadata.fork_tick {
            toml_content.push_str(&format!("# Fork tick: {}\n", fork_tick));
        }

        Ok(toml_content)
    }

    /// Serialize a single action to TOML
    fn serialize_action_to_toml(
        &self,
        content: &mut String,
        action: &ScenarioAction,
    ) -> Result<()> {
        match action {
            ScenarioAction::Step { count, at_tick } => {
                content.push_str("type = \"Step\"\n");
                content.push_str(&format!("count = {}\n", count));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::StepUntil {
                condition,
                at_tick,
                max_steps,
            } => {
                content.push_str("type = \"StepUntil\"\n");
                content.push_str(&format!("condition = \"{}\"\n", condition));
                content.push_str(&format!("at_tick = {}\n", at_tick));
                if let Some(max) = max_steps {
                    content.push_str(&format!("max_steps = {}\n", max));
                }
            }
            ScenarioAction::InitiateDkd {
                participants,
                app_id,
                context,
                at_tick,
            } => {
                content.push_str("type = \"InitiateDkd\"\n");
                content.push_str("participants = [");
                for (i, p) in participants.iter().enumerate() {
                    if i > 0 {
                        content.push_str(", ");
                    }
                    content.push_str(&format!("\"{}\"", p));
                }
                content.push_str("]\n");
                content.push_str(&format!("app_id = \"{}\"\n", app_id));
                content.push_str(&format!("context = \"{}\"\n", context));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::InitiateRecovery {
                participants,
                recovery_data,
                at_tick,
            } => {
                content.push_str("type = \"InitiateRecovery\"\n");
                content.push_str("participants = [");
                for (i, p) in participants.iter().enumerate() {
                    if i > 0 {
                        content.push_str(", ");
                    }
                    content.push_str(&format!("\"{}\"", p));
                }
                content.push_str("]\n");
                content.push_str(&format!("recovery_data = \"{}\"\n", recovery_data));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::Partition {
                participants,
                at_tick,
            } => {
                content.push_str("type = \"Partition\"\n");
                content.push_str("participants = [");
                for (i, p) in participants.iter().enumerate() {
                    if i > 0 {
                        content.push_str(", ");
                    }
                    content.push_str(&format!("\"{}\"", p));
                }
                content.push_str("]\n");
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::Heal { at_tick } => {
                content.push_str("type = \"Heal\"\n");
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::Delay {
                from,
                to,
                delay_ms,
                at_tick,
            } => {
                content.push_str("type = \"Delay\"\n");
                content.push_str(&format!("from = \"{}\"\n", from));
                content.push_str(&format!("to = \"{}\"\n", to));
                content.push_str(&format!("delay_ms = {}\n", delay_ms));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::Byzantine {
                participant,
                strategy,
                at_tick,
            } => {
                content.push_str("type = \"Byzantine\"\n");
                content.push_str(&format!("participant = \"{}\"\n", participant));
                content.push_str(&format!("strategy = \"{}\"\n", strategy));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
            ScenarioAction::Inject {
                participant,
                event_type,
                at_tick,
            } => {
                content.push_str("type = \"Inject\"\n");
                content.push_str(&format!("participant = \"{}\"\n", participant));
                content.push_str(&format!("event_type = \"{}\"\n", event_type));
                content.push_str(&format!("at_tick = {}\n", at_tick));
            }
        }
        Ok(())
    }

    /// Clear command history (useful for testing)
    pub fn clear_history(&mut self) {
        self.command_history.clear();
        self.next_sequence = 0;
    }

    /// Get command history length
    pub fn history_length(&self) -> usize {
        self.command_history.len()
    }
}

impl Default for ScenarioExporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_console_types::*;

    #[test]
    fn test_scenario_exporter_creation() {
        let exporter = ScenarioExporter::new();
        assert_eq!(exporter.history_length(), 0);
    }

    #[test]
    fn test_command_recording() {
        let mut exporter = ScenarioExporter::new();

        let command = ConsoleCommand::Step { count: 5 };
        exporter.record_command(command, 0);

        assert_eq!(exporter.history_length(), 1);
    }

    #[test]
    fn test_command_to_action_conversion() {
        let exporter = ScenarioExporter::new();

        let step_command = ConsoleCommand::Step { count: 3 };
        let action = exporter
            .convert_command_to_action(&step_command, 10)
            .unwrap();

        match action {
            Some(ScenarioAction::Step { count, at_tick }) => {
                assert_eq!(count, 3);
                assert_eq!(at_tick, 10);
            }
            _ => panic!("Expected Step action"),
        }
    }

    #[test]
    fn test_readonly_commands_no_action() {
        let exporter = ScenarioExporter::new();

        let readonly_command = ConsoleCommand::GetTopology;
        let action = exporter
            .convert_command_to_action(&readonly_command, 5)
            .unwrap();

        assert!(
            action.is_none(),
            "Read-only commands should not generate actions"
        );
    }

    #[test]
    fn test_toml_serialization_basic() {
        let scenario = ExportedScenario {
            name: "test_scenario".to_string(),
            description: Some("Test scenario".to_string()),
            setup: ScenarioSetup {
                participant_count: 3,
                threshold: 2,
                protocol: "Dkd".to_string(),
                network_conditions: None,
                byzantine_conditions: None,
                timeout_ms: Some(30000),
                max_ticks: Some(1000),
            },
            actions: vec![
                ScenarioAction::Step {
                    count: 5,
                    at_tick: 0,
                },
                ScenarioAction::InitiateDkd {
                    participants: vec!["alice".to_string(), "bob".to_string()],
                    app_id: "test_app".to_string(),
                    context: "test_context".to_string(),
                    at_tick: 5,
                },
            ],
            assertions: vec![],
            metadata: ExportMetadata {
                source_branch_id: "test-branch".to_string(),
                exported_at: current_timestamp,
                parent_branch_id: None,
                fork_tick: None,
                seed: 42,
                exported_by: "test".to_string(),
            },
        };

        let exporter = ScenarioExporter::new();
        let toml_output = exporter.serialize_to_toml(scenario).unwrap();

        assert!(toml_output.contains("name = \"test_scenario\""));
        assert!(toml_output.contains("participant_count = 3"));
        assert!(toml_output.contains("threshold = 2"));
        assert!(toml_output.contains("type = \"Step\""));
        assert!(toml_output.contains("type = \"InitiateDkd\""));
    }
}

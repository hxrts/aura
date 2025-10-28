//! Command handler for processing REPL commands
//!
//! Processes all ConsoleCommand variants and executes them against simulation branches,
//! providing comprehensive control and inspection capabilities for distributed protocols.

use anyhow::{anyhow, Result};
use aura_console_types::{
    BranchInfo, ConsoleCommand, ConsoleResponse, DeviceInfo, EventType, LedgerStateInfo,
    SimulationInfo, TraceEvent,
};
use crate::simulation_wrapper::SimulationWrapper;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};
use uuid::Uuid;
use uuid::Uuid;

use crate::branch_manager::{BranchId, BranchManager};

/// Command handler for processing REPL commands against simulation branches
pub struct CommandHandler {
    /// Reference to the branch manager
    branch_manager: Arc<Mutex<BranchManager>>,
}

impl CommandHandler {
    /// Create a new command handler
    pub fn new(branch_manager: Arc<Mutex<BranchManager>>) -> Self {
        Self { branch_manager }
    }

    /// Execute a command against a specific branch
    pub async fn execute_command(
        &self,
        command: ConsoleCommand,
        branch_id: BranchId,
    ) -> Result<ConsoleResponse> {
        debug!("Executing command {:?} on branch {}", command, branch_id);

        // Get current tick for command recording
        let current_tick = {
            let mut branch_manager = self.branch_manager.lock().unwrap();
            if let Some(branch) = branch_manager.get_branch(branch_id) {
                let simulation = branch.simulation.lock().unwrap();
                simulation.current_tick()
            } else {
                0
            }
        };

        // Record command execution for scenario export (only for mutation commands)
        if self.is_mutation_command(&command) {
            let mut branch_manager = self.branch_manager.lock().unwrap();
            branch_manager.record_command_execution(branch_id, command.clone(), current_tick);
        }

        match command {
            // Read-only commands
            ConsoleCommand::QueryState { device_id } => {
                self.handle_query_state(branch_id, device_id).await
            }
            ConsoleCommand::GetTopology => self.handle_get_topology(branch_id).await,
            ConsoleCommand::GetLedger { device_id } => {
                self.handle_get_ledger(branch_id, device_id).await
            }
            ConsoleCommand::GetViolations => self.handle_get_violations(branch_id).await,
            ConsoleCommand::ListBranches => self.handle_list_branches().await,

            // Mutation commands (require forking)
            ConsoleCommand::Step { count } => self.handle_step(branch_id, count).await,
            ConsoleCommand::RunUntilIdle => self.handle_run_until_idle(branch_id).await,
            ConsoleCommand::SeekToTick { tick } => self.handle_seek_to_tick(branch_id, tick).await,

            // Checkpoint commands
            ConsoleCommand::Checkpoint { label } => self.handle_checkpoint(branch_id, label).await,
            ConsoleCommand::RestoreCheckpoint { checkpoint_id } => {
                self.handle_restore_checkpoint(branch_id, checkpoint_id)
                    .await
            }

            // Protocol commands
            ConsoleCommand::InitiateDkd {
                participants,
                context,
            } => {
                self.handle_initiate_dkd(branch_id, participants, context)
                    .await
            }
            ConsoleCommand::InitiateResharing { participants } => {
                self.handle_initiate_resharing(branch_id, participants)
                    .await
            }
            ConsoleCommand::InitiateRecovery { guardians } => {
                self.handle_initiate_recovery(branch_id, guardians).await
            }

            // Network simulation commands
            ConsoleCommand::CreatePartition { devices } => {
                self.handle_create_partition(branch_id, devices).await
            }
            ConsoleCommand::SetDeviceOffline { device_id } => {
                self.handle_set_device_offline(branch_id, device_id).await
            }
            ConsoleCommand::EnableByzantine {
                device_id,
                strategy,
            } => {
                self.handle_enable_byzantine(branch_id, device_id, strategy)
                    .await
            }

            // Message injection
            ConsoleCommand::InjectMessage { to, message } => {
                self.handle_inject_message(branch_id, to, message).await
            }
            ConsoleCommand::BroadcastMessage { message } => {
                self.handle_broadcast_message(branch_id, message).await
            }

            // Branch management
            ConsoleCommand::CheckoutBranch {
                branch_id: target_branch_id,
            } => self.handle_checkout_branch(target_branch_id).await,
            ConsoleCommand::ForkBranch { label } => self.handle_fork_branch(branch_id, label).await,
            ConsoleCommand::DeleteBranch {
                branch_id: target_branch_id,
            } => self.handle_delete_branch(target_branch_id).await,
            ConsoleCommand::ExportScenario {
                branch_id: export_branch_id,
                filename,
            } => {
                self.handle_export_scenario(export_branch_id, filename)
                    .await
            }

            // Scenario management
            ConsoleCommand::LoadScenario { filename } => {
                self.handle_load_scenario(branch_id, filename).await
            }
            ConsoleCommand::LoadTrace { filename } => {
                self.handle_load_trace(branch_id, filename).await
            }

            // Analysis
            ConsoleCommand::GetCausalityPath { event_id } => {
                self.handle_get_causality_path(branch_id, event_id).await
            }
            ConsoleCommand::GetEventsInRange { start, end } => {
                self.handle_get_events_in_range(branch_id, start, end).await
            }
        }
    }

    /// Handle help command
    async fn handle_help(&self) -> Result<ConsoleResponse> {
        let help_text = r#"
Aura Dev Console Commands:

General:
  help           - Show this help message
  status         - Show simulation status
  devices        - List devices and their states
  state          - Show current simulation state
  ledger         - Show ledger state
  branches       - List all simulation branches
  events [since] - Show trace events (optionally since event ID)

Control:
  step [count]   - Step simulation forward (default: 1)
  step-until <condition> - Step until condition is met
  reset          - Reset simulation to initial state
  fork [name]    - Create a new branch from current state
  switch <branch> - Switch to a different branch

Protocols:
  initiate-dkd <participants> <app_id> <context> - Start DKD session
  initiate-recovery <participants> <recovery_data> - Start recovery session

Network:
  partition <participants> - Create network partition
  heal          - Remove all network partitions
  delay <from> <to> <ms> - Add message delay between participants

Testing:
  byzantine <participant> <strategy> - Make participant byzantine
  inject <participant> <event> - Inject custom event
"#;

        Ok(ConsoleResponse::Help {
            help_text: help_text.to_string(),
        })
    }

    /// Handle status command
    async fn handle_status(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let simulation = branch.simulation.lock().unwrap();

            let simulation_info = SimulationInfo {
                id: simulation.id,
                current_tick: simulation.current_tick(),
                current_time: simulation.current_time(),
                seed: simulation.seed,
                is_recording: simulation.is_recording_enabled(),
            };

            Ok(ConsoleResponse::Status { simulation_info })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle devices command
    async fn handle_devices(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let simulation = branch.simulation.lock().unwrap();
            let participants = simulation.get_participants();

            let devices: Vec<DeviceInfo> = participants
                .iter()
                .map(|(id, participant)| DeviceInfo {
                    id: id.clone(),
                    device_id: participant.device_id.clone(),
                    account_id: participant.account_id.clone(),
                    participant_type: participant.participant_type,
                    status: participant.status,
                    message_count: participant.message_count,
                })
                .collect();

            Ok(ConsoleResponse::Devices { devices })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle state command  
    async fn handle_state(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let simulation = branch.simulation.lock().unwrap();

            // For now, return a simple JSON representation
            // In a full implementation, this would include detailed protocol states
            let state_json = serde_json::json!({
                "tick": simulation.current_tick(),
                "time": simulation.current_time(),
                "participants": simulation.get_participants().len(),
                "seed": simulation.seed
            });

            Ok(ConsoleResponse::State {
                state: state_json.to_string(),
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle ledger command
    async fn handle_ledger(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let simulation = branch.simulation.lock().unwrap();

            // For now, return basic ledger info
            // In a full implementation, this would show the actual CRDT state
            let ledger_info = LedgerStateInfo {
                head_count: 1, // Placeholder
                total_events: simulation.current_tick(),
                participants: simulation.get_participants().len() as u64,
                latest_sequence: simulation.current_tick(),
            };

            Ok(ConsoleResponse::Ledger { ledger_info })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle branches command
    async fn handle_branches(&self) -> Result<ConsoleResponse> {
        let branch_manager = self.branch_manager.lock().unwrap();
        let branches = branch_manager.list_branches();

        Ok(ConsoleResponse::Branches { branches })
    }

    /// Handle events command
    async fn handle_events(
        &self,
        branch_id: BranchId,
        since: Option<u64>,
    ) -> Result<ConsoleResponse> {
        let branch_manager = self.branch_manager.lock().unwrap();
        let events = branch_manager.get_branch_events(branch_id, since);

        Ok(ConsoleResponse::Events { events })
    }

    /// Handle step command
    async fn handle_step(
        &self,
        branch_id: BranchId,
        count: Option<u64>,
    ) -> Result<ConsoleResponse> {
        let step_count = count.unwrap_or(1);

        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            for _ in 0..step_count {
                simulation.step()?;
            }

            info!(
                "Stepped simulation {} steps to tick {}",
                step_count,
                simulation.current_tick()
            );

            Ok(ConsoleResponse::Step {
                new_tick: simulation.current_tick(),
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle step-until command
    async fn handle_step_until(
        &self,
        branch_id: BranchId,
        condition: String,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            let max_steps = 1000; // Safety limit
            let mut steps = 0;

            // Simple condition checking - in a full implementation, this would parse and evaluate complex conditions
            while steps < max_steps {
                simulation.step()?;
                steps += 1;

                // Check condition (placeholder logic)
                if condition == "idle" && simulation.is_idle() {
                    break;
                }

                // Check for specific tick conditions
                if condition.starts_with("tick=") {
                    if let Ok(target_tick) = condition[5..].parse::<u64>() {
                        if simulation.current_tick() >= target_tick {
                            break;
                        }
                    }
                }
            }

            info!(
                "Stepped {} steps to tick {} (condition: {})",
                steps,
                simulation.current_tick(),
                condition
            );

            Ok(ConsoleResponse::StepUntil {
                final_tick: simulation.current_tick(),
                steps_taken: steps,
                condition_met: steps < max_steps,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle reset command
    async fn handle_reset(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let original_seed = {
                let simulation = branch.simulation.lock().unwrap();
                simulation.seed
            };

            // Create a new simulation with the same seed
            let new_simulation = SimulationWrapper::new(original_seed);
            *branch.simulation.lock().unwrap() = new_simulation;

            info!("Reset simulation on branch {}", branch_id);

            Ok(ConsoleResponse::Reset)
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle fork command
    async fn handle_fork(
        &self,
        branch_id: BranchId,
        name: Option<String>,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();
        let new_branch_id = branch_manager.fork_branch(branch_id, name)?;

        info!("Forked branch {} from {}", new_branch_id, branch_id);

        Ok(ConsoleResponse::Fork {
            new_branch_id,
            parent_branch_id: branch_id,
        })
    }

    /// Handle switch command
    async fn handle_switch(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let branch_manager = self.branch_manager.lock().unwrap();

        if branch_manager.get_branch_info(branch_id).is_some() {
            Ok(ConsoleResponse::Switch {
                new_branch_id: branch_id,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle initiate-dkd command
    async fn handle_initiate_dkd(
        &self,
        branch_id: BranchId,
        participants: Vec<String>,
        app_id: String,
        context: String,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Add participants if they don't exist
            for participant_id in &participants {
                if simulation.get_participant(participant_id).is_none() {
                    simulation.add_participant(
                        participant_id.clone(),
                        format!("device_{}", participant_id),
                        format!("account_{}", participant_id),
                    )?;
                }
            }

            // Record DKD initiation event
            simulation.record_state_transition(
                "coordinator".to_string(),
                "DKD".to_string(),
                "idle".to_string(),
                "initiated".to_string(),
                Some(format!("app_id:{},context:{}", app_id, context).into_bytes()),
            );

            info!("Initiated DKD with participants {:?}", participants);

            Ok(ConsoleResponse::InitiateDkd {
                session_id: Uuid::new_v4().to_string(),
                participants,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle initiate-recovery command
    async fn handle_initiate_recovery(
        &self,
        branch_id: BranchId,
        participants: Vec<String>,
        recovery_data: String,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Record recovery initiation event
            simulation.record_state_transition(
                "coordinator".to_string(),
                "Recovery".to_string(),
                "idle".to_string(),
                "initiated".to_string(),
                Some(recovery_data.into_bytes()),
            );

            info!("Initiated recovery with participants {:?}", participants);

            Ok(ConsoleResponse::InitiateRecovery {
                session_id: Uuid::new_v4().to_string(),
                participants,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle partition command
    async fn handle_partition(
        &self,
        branch_id: BranchId,
        participants: Vec<String>,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Record partition event
            simulation.record_effect_executed(
                "network".to_string(),
                "partition_created".to_string(),
                participants.join(",").into_bytes(),
            );

            info!("Created network partition: {:?}", participants);

            Ok(ConsoleResponse::Partition { participants })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle heal command
    async fn handle_heal(&self, branch_id: BranchId) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Record heal event
            simulation.record_effect_executed(
                "network".to_string(),
                "partitions_healed".to_string(),
                vec![],
            );

            info!("Healed all network partitions");

            Ok(ConsoleResponse::Heal)
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle delay command
    async fn handle_delay(
        &self,
        branch_id: BranchId,
        from: String,
        to: String,
        delay_ms: u64,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Record delay event
            simulation.record_effect_executed(
                "network".to_string(),
                "delay_added".to_string(),
                format!("{}->{}:{}ms", from, to, delay_ms).into_bytes(),
            );

            info!("Added {}ms delay from {} to {}", delay_ms, from, to);

            Ok(ConsoleResponse::Delay { from, to, delay_ms })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle byzantine command
    async fn handle_byzantine(
        &self,
        branch_id: BranchId,
        participant: String,
        strategy: String,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Set participant as byzantine
            simulation.set_participant_byzantine(&participant)?;

            // Record byzantine event
            simulation.record_effect_executed(
                participant.clone(),
                "byzantine_enabled".to_string(),
                strategy.clone().into_bytes(),
            );

            info!(
                "Set participant {} as byzantine with strategy {}",
                participant, strategy
            );

            Ok(ConsoleResponse::Byzantine {
                participant,
                strategy,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle inject command
    async fn handle_inject(
        &self,
        branch_id: BranchId,
        participant: String,
        event_type: String,
    ) -> Result<ConsoleResponse> {
        let mut branch_manager = self.branch_manager.lock().unwrap();

        if let Some(branch) = branch_manager.get_branch(branch_id) {
            let mut simulation = branch.simulation.lock().unwrap();

            // Inject custom event
            simulation.record_effect_executed(
                participant.clone(),
                format!("injected_{}", event_type),
                vec![],
            );

            info!(
                "Injected {} event for participant {}",
                event_type, participant
            );

            Ok(ConsoleResponse::Inject {
                participant,
                event_type,
            })
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Handle export scenario command
    async fn handle_export_scenario(
        &self,
        export_branch_id: String,
        filename: String,
    ) -> Result<ConsoleResponse> {
        let branch_id = Uuid::parse_str(&export_branch_id)
            .map_err(|_| anyhow!("Invalid branch ID format: {}", export_branch_id))?;

        let branch_manager = self.branch_manager.lock().unwrap();

        // Extract scenario name and description from filename
        let scenario_name = if filename.ends_with(".toml") {
            filename
                .strip_suffix(".toml")
                .unwrap_or(&filename)
                .to_string()
        } else {
            filename.clone()
        };

        let description = Some(format!("Exported scenario from branch {}", branch_id));

        let toml_content = branch_manager.export_branch_as_scenario(
            branch_id,
            Some(scenario_name.clone()),
            description,
        )?;

        info!(
            "Exported scenario '{}' from branch {} ({} bytes)",
            scenario_name,
            branch_id,
            toml_content.len()
        );

        Ok(ConsoleResponse::ExportScenario {
            toml_content,
            filename,
        })
    }

    /// Check if a command is a mutation command that should be recorded for scenario export
    fn is_mutation_command(&self, command: &ConsoleCommand) -> bool {
        match command {
            // Mutation commands that change simulation state
            ConsoleCommand::Step { .. }
            | ConsoleCommand::StepUntil { .. }
            | ConsoleCommand::InitiateDkd { .. }
            | ConsoleCommand::InitiateRecovery { .. }
            | ConsoleCommand::Partition { .. }
            | ConsoleCommand::Heal
            | ConsoleCommand::Delay { .. }
            | ConsoleCommand::Byzantine { .. }
            | ConsoleCommand::Inject { .. } => true,

            // Read-only and control commands don't get recorded
            ConsoleCommand::Help
            | ConsoleCommand::Status
            | ConsoleCommand::Devices
            | ConsoleCommand::State
            | ConsoleCommand::Ledger
            | ConsoleCommand::Branches
            | ConsoleCommand::Events { .. }
            | ConsoleCommand::Reset
            | ConsoleCommand::Fork { .. }
            | ConsoleCommand::Switch { .. }
            | ConsoleCommand::ExportScenario { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch_manager::BranchManager;

    #[tokio::test]
    async fn test_command_handler_creation() {
        let branch_manager = Arc::new(Mutex::new(BranchManager::new()));
        let handler = CommandHandler::new(branch_manager);

        // Test help command
        let response = handler
            .execute_command(ConsoleCommand::Help, Uuid::new_v4())
            .await
            .unwrap();

        match response {
            ConsoleResponse::Help { help_text } => {
                assert!(help_text.contains("Aura Dev Console Commands"));
            }
            _ => panic!("Expected Help response"),
        }
    }

    #[tokio::test]
    async fn test_status_command() {
        let branch_manager = Arc::new(Mutex::new(BranchManager::new()));
        let branch_id = {
            let mut manager = branch_manager.lock().unwrap();
            manager.get_or_create_default_branch()
        };

        let handler = CommandHandler::new(branch_manager);

        let response = handler
            .execute_command(ConsoleCommand::Status, branch_id)
            .await
            .unwrap();

        match response {
            ConsoleResponse::Status { simulation_info } => {
                assert_eq!(simulation_info.current_tick, 0);
                assert_eq!(simulation_info.current_time, 0);
            }
            _ => panic!("Expected Status response"),
        }
    }

    #[tokio::test]
    async fn test_step_command() {
        let branch_manager = Arc::new(Mutex::new(BranchManager::new()));
        let branch_id = {
            let mut manager = branch_manager.lock().unwrap();
            manager.get_or_create_default_branch()
        };

        let handler = CommandHandler::new(branch_manager);

        let response = handler
            .execute_command(ConsoleCommand::Step { count: Some(5) }, branch_id)
            .await
            .unwrap();

        match response {
            ConsoleResponse::Step { new_tick } => {
                assert_eq!(new_tick, 5);
            }
            _ => panic!("Expected Step response"),
        }
    }
}

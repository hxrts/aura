//! Simplified command handler for basic console functionality

use anyhow::{anyhow, Result};
use aura_console_types::{ConsoleCommand, ConsoleResponse};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

use crate::branch_manager::{BranchId, BranchManager};

/// Simplified command handler that implements core console commands
pub struct SimpleCommandHandler {
    branch_manager: Arc<Mutex<BranchManager>>,
}

impl SimpleCommandHandler {
    pub fn new(branch_manager: Arc<Mutex<BranchManager>>) -> Self {
        Self { branch_manager }
    }

    pub async fn execute_command(
        &self,
        command: ConsoleCommand,
        branch_id: BranchId,
    ) -> Result<ConsoleResponse> {
        debug!("Executing command {:?} on branch {}", command, branch_id);

        match command {
            ConsoleCommand::Step { count } => {
                let mut branch_manager = self.branch_manager.lock().unwrap();
                if let Some(branch) = branch_manager.get_branch(branch_id) {
                    let mut simulation = branch.simulation.lock().unwrap();
                    for _ in 0..count {
                        simulation.step()?;
                    }
                    // Record the command for scenario export
                    drop(simulation);
                    branch_manager.record_command_execution(
                        branch_id,
                        ConsoleCommand::Step { count },
                        0, // Current tick would be fetched properly in full implementation
                    );
                    Ok(ConsoleResponse::ExportScenario {
                        toml_content: "Step command executed".to_string(),
                        filename: "response".to_string(),
                    })
                } else {
                    Err(anyhow!("Branch not found"))
                }
            }

            ConsoleCommand::ExportScenario {
                branch_id: export_branch_id,
                filename,
            } => {
                let branch_id = uuid::Uuid::parse_str(&export_branch_id)
                    .map_err(|_| anyhow!("Invalid branch ID: {}", export_branch_id))?;

                let branch_manager = self.branch_manager.lock().unwrap();
                let toml_content = branch_manager.export_branch_as_scenario(
                    branch_id,
                    Some("exported_scenario".to_string()),
                    Some("Exported from dev console".to_string()),
                )?;

                info!(
                    "Exported scenario from branch {} to {}",
                    branch_id, filename
                );

                Ok(ConsoleResponse::ExportScenario {
                    toml_content,
                    filename,
                })
            }

            // Stub implementations for other commands
            _ => Ok(ConsoleResponse::ExportScenario {
                toml_content: format!("Command {:?} not implemented yet", command),
                filename: "stub".to_string(),
            }),
        }
    }
}

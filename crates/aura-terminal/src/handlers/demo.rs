//! # Demo Handler
//!
//! Handler for demo-related CLI commands.

use aura_core::AuraError;
use std::path::PathBuf;

use crate::cli::tui::TuiArgs;
use crate::handlers::tui::handle_tui;
use crate::ids;
use crate::{
    cli::demo::{DemoCommands, DemoScenarioArg},
    create_cli_handler, ScenarioAction,
};

/// Handler for demo commands
pub struct DemoHandler;

impl DemoHandler {
    /// Handle demo commands
    pub async fn handle_demo_command(command: DemoCommands) -> Result<(), AuraError> {
        match command {
            DemoCommands::RecoveryWorkflow {
                directory,
                seed,
                detailed_report,
            } => Self::handle_recovery_workflow(directory, seed, detailed_report).await,

            DemoCommands::Tui { scenario } => Self::handle_tui_demo(scenario).await,

            DemoCommands::HumanAgent { scenario } => {
                // Human-agent demo uses the TUI with a focus on agent conversation
                // Currently routes to the same handler until full implementation is ready
                Self::handle_tui_demo(scenario).await
            }
        }
    }

    async fn handle_recovery_workflow(
        directory: Option<PathBuf>,
        seed: u64,
        detailed_report: bool,
    ) -> Result<(), AuraError> {
        let directory = directory.unwrap_or_else(|| PathBuf::from("scenarios"));
        println!("Running Bob recovery workflow via scenario runner");
        println!("Scenario root: {}", directory.display());
        println!("Seed: {}", seed);
        println!("Detailed report: {}", detailed_report);

        // Build a CLI handler with a deterministic device/authority context
        let handler =
            create_cli_handler(ids::device_id(&format!("demo:recovery-workflow:{}", seed)))
                .map_err(|e| {
                    AuraError::internal(format!("Failed to create demo handler: {}", e))
                })?;

        // Execute the cli_recovery_demo scenario via existing scenario machinery
        handler
            .handle_scenarios(&ScenarioAction::Run {
                directory: Some(directory),
                pattern: Some("cli_recovery_demo".into()),
                parallel: false,
                max_parallel: Some(1),
                output_file: None,
                detailed_report,
            })
            .await
            .map_err(|e| AuraError::internal(format!("Recovery workflow failed: {}", e)))
    }

    /// Handle TUI demo command
    ///
    /// Routes to the TUI handler with demo mode enabled.
    /// The TUI code is IDENTICAL for demo and production - only the backend differs.
    async fn handle_tui_demo(scenario_arg: DemoScenarioArg) -> Result<(), AuraError> {
        // Map scenario arg to appropriate demo configuration
        // Each scenario configures different behavior for Alice/Charlie peer agents
        let (data_dir, device_id, scenario_name) = match scenario_arg {
            DemoScenarioArg::HappyPath => {
                // Happy path - guardians respond quickly
                (
                    "./aura-demo-happy".to_string(),
                    "demo:bob:happy".to_string(),
                    "happy_path",
                )
            }
            DemoScenarioArg::SlowGuardian => {
                // One guardian is slow to respond
                (
                    "./aura-demo-slow".to_string(),
                    "demo:bob:slow".to_string(),
                    "slow_guardian",
                )
            }
            DemoScenarioArg::FailedRecovery => {
                // Recovery fails (for error handling demo)
                (
                    "./aura-demo-failed".to_string(),
                    "demo:bob:failed".to_string(),
                    "failed_recovery",
                )
            }
            DemoScenarioArg::Interactive => {
                // Interactive demo for free-form exploration
                (
                    "./aura-demo-interactive".to_string(),
                    "demo:bob:interactive".to_string(),
                    "interactive",
                )
            }
        };

        println!("Starting demo scenario: {}", scenario_name);

        // Construct TuiArgs with demo mode enabled
        let tui_args = TuiArgs {
            data_dir: Some(data_dir),
            device_id: Some(device_id),
            demo: true,
        };

        // Route to TUI handler - it uses the same code path for demo and production
        handle_tui(&tui_args)
            .await
            .map_err(|e| AuraError::internal(format!("TUI demo failed: {}", e)))
    }
}

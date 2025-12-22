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
    ///
    /// All scenarios use the standard `.aura-demo` directory (via `resolve_storage_path`).
    /// The directory is cleaned up on each demo run, so scenario isolation isn't needed.
    async fn handle_tui_demo(scenario_arg: DemoScenarioArg) -> Result<(), AuraError> {
        // Map scenario arg to device ID for scenario-specific behavior
        let (device_id, scenario_name) = match scenario_arg {
            DemoScenarioArg::HappyPath => ("demo:bob:happy", "happy_path"),
            DemoScenarioArg::SlowGuardian => ("demo:bob:slow", "slow_guardian"),
            DemoScenarioArg::FailedRecovery => ("demo:bob:failed", "failed_recovery"),
            DemoScenarioArg::Interactive => ("demo:bob:interactive", "interactive"),
        };

        println!("Starting demo scenario: {}", scenario_name);

        // Construct TuiArgs with demo mode enabled
        // data_dir: None means resolve_storage_path will use $AURA_PATH/.aura-demo
        let tui_args = TuiArgs {
            data_dir: None,
            device_id: Some(device_id.to_string()),
            demo: true,
        };

        // Route to TUI handler - it uses the same code path for demo and production
        handle_tui(&tui_args)
            .await
            .map_err(|e| AuraError::internal(format!("TUI demo failed: {}", e)))
    }
}

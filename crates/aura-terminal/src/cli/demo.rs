//! # Demo Commands
//!
//! CLI commands for running Aura capability demonstrations.

use clap::Subcommand;
use std::path::PathBuf;

/// Demo-related subcommands
#[derive(Debug, Clone, Subcommand)]
pub enum DemoCommands {
    /// Run the CLI recovery demo workflow (Bob + guardians)
    RecoveryWorkflow {
        /// Scenario root directory (defaults to bundled scenarios/)
        #[arg(long)]
        directory: Option<PathBuf>,
        /// Deterministic seed for simulation
        #[arg(long, default_value = "2024")]
        seed: u64,
        /// Emit detailed simulator report
        #[arg(long)]
        detailed_report: bool,
    },

    /// Run the TUI demo with simulated backend
    ///
    /// This runs the real TUI with a simulated backend, allowing you to
    /// explore the interface without needing a real Aura network. Contextual
    /// tips guide you through the demo flow.
    Tui {
        /// Demo scenario to run
        #[arg(long, value_enum, default_value = "happy-path")]
        scenario: DemoScenarioArg,
    },

    /// Run the interactive human-agent conversation demo
    ///
    /// This demo showcases conversation-style interaction between a human
    /// and an AI agent in the TUI. It demonstrates the AMP (Agent Message
    /// Protocol) flow with simulated agent responses.
    HumanAgent {
        /// Demo scenario to run
        #[arg(long, value_enum, default_value = "interactive")]
        scenario: DemoScenarioArg,
    },
}

/// Demo scenario argument for CLI
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum DemoScenarioArg {
    /// Happy path - guardians respond quickly
    #[default]
    HappyPath,
    /// One guardian is slow to respond
    SlowGuardian,
    /// Recovery fails (for error handling demo)
    FailedRecovery,
    /// Interactive - no auto-responses, user triggers everything
    Interactive,
}

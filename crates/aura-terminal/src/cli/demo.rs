//! # Demo Commands
//!
//! CLI commands for running Aura capability demonstrations.

use bpaf::{command, construct, long, Parser};
use std::path::PathBuf;
use std::str::FromStr;

/// Demo-related subcommands
#[derive(Debug, Clone)]
pub enum DemoCommands {
    /// Run the CLI recovery demo workflow (Bob + guardians)
    RecoveryWorkflow {
        /// Scenario root directory (defaults to bundled scenarios/)
        directory: Option<PathBuf>,
        /// Deterministic seed for simulation
        seed: u64,
        /// Emit detailed simulator report
        detailed_report: bool,
    },

    /// Run the TUI demo with simulated backend
    ///
    /// This runs the real TUI with a simulated backend, allowing you to
    /// explore the interface without needing a real Aura network. Contextual
    /// tips guide you through the demo flow.
    Tui {
        /// Demo scenario to run
        scenario: DemoScenarioArg,
    },

    /// Run the interactive human-agent conversation demo
    ///
    /// This demo showcases conversation-style interaction between a human
    /// and an AI agent in the TUI. It demonstrates the AMP (Agent Message
    /// Protocol) flow with simulated agent responses.
    HumanAgent {
        /// Demo scenario to run
        scenario: DemoScenarioArg,
    },
}

/// Demo scenario argument for CLI
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

impl FromStr for DemoScenarioArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "happy-path" => Ok(DemoScenarioArg::HappyPath),
            "slow-guardian" => Ok(DemoScenarioArg::SlowGuardian),
            "failed-recovery" => Ok(DemoScenarioArg::FailedRecovery),
            "interactive" => Ok(DemoScenarioArg::Interactive),
            other => Err(format!(
                "Invalid scenario '{}'. Expected one of: happy-path, slow-guardian, failed-recovery, interactive",
                other
            )),
        }
    }
}

fn recovery_workflow_command() -> impl Parser<DemoCommands> {
    command(
        "recovery-workflow",
        construct!(DemoCommands::RecoveryWorkflow {
            directory: long("directory")
                .help("Scenario root directory (defaults to bundled scenarios/)")
                .argument::<PathBuf>("DIR")
                .optional(),
            seed: long("seed")
                .help("Deterministic seed for simulation")
                .argument::<u64>("SEED")
                .fallback(2024),
            detailed_report: long("detailed-report")
                .help("Emit detailed simulator report")
                .switch(),
        }),
    )
    .help("Run the CLI recovery demo workflow (Bob + guardians)")
}

fn tui_demo_command() -> impl Parser<DemoCommands> {
    command(
        "tui",
        construct!(DemoCommands::Tui {
            scenario: long("scenario")
                .help("Demo scenario to run")
                .argument::<DemoScenarioArg>("SCENARIO")
                .fallback(DemoScenarioArg::HappyPath),
        }),
    )
    .help("Run the TUI demo with simulated backend")
}

fn human_agent_command() -> impl Parser<DemoCommands> {
    command(
        "human-agent",
        construct!(DemoCommands::HumanAgent {
            scenario: long("scenario")
                .help("Demo scenario to run")
                .argument::<DemoScenarioArg>("SCENARIO")
                .fallback(DemoScenarioArg::Interactive),
        }),
    )
    .help("Run the interactive human-agent conversation demo")
}

pub fn demo_parser() -> impl Parser<DemoCommands> {
    construct!([
        recovery_workflow_command(),
        tui_demo_command(),
        human_agent_command()
    ])
}

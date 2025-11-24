//! # Demo Commands
//!
//! CLI commands for running Aura capability demonstrations.

use clap::Subcommand;
use std::path::PathBuf;

/// Demo-related subcommands
#[derive(Debug, Clone, Subcommand)]
pub enum DemoCommands {
    /// Run the human-agent recovery demo
    HumanAgent {
        /// Deterministic seed for reproducible demo
        #[arg(long, default_value = "42")]
        seed: u64,

        /// Enable verbose logging and slower progression
        #[arg(long)]
        verbose: bool,

        /// Auto-advance through demo phases
        #[arg(long, default_value = "true")]
        auto_advance: bool,

        /// Demo timeout in minutes
        #[arg(long, default_value = "15")]
        timeout_minutes: u64,

        /// Guardian response delay in milliseconds
        #[arg(long, default_value = "3000")]
        guardian_delay_ms: u64,

        /// Save session recording to file
        #[arg(long)]
        record_to: Option<PathBuf>,
    },

    /// Run the orchestrator in interactive mode
    Orchestrator {
        /// Configuration seed
        #[arg(long, default_value = "42")]
        seed: u64,

        /// Enable session recording
        #[arg(long)]
        record_sessions: bool,

        /// Maximum concurrent sessions
        #[arg(long, default_value = "1")]
        max_sessions: usize,
    },

    /// View demo statistics and history
    Stats {
        /// Show detailed statistics
        #[arg(long)]
        detailed: bool,

        /// Export statistics to file
        #[arg(long)]
        export_to: Option<PathBuf>,
    },

    /// Scenario-driven demo setup
    Scenario {
        /// Setup configuration file
        #[arg(long)]
        config: Option<PathBuf>,

        /// Participant count
        #[arg(long, default_value = "3")]
        participants: usize,

        /// Guardian threshold
        #[arg(long, default_value = "2")]
        threshold: usize,

        /// Setup chat history
        #[arg(long)]
        setup_chat: bool,

        /// Number of initial messages
        #[arg(long, default_value = "5")]
        initial_messages: usize,

        /// Run setup only (don't start demo)
        #[arg(long)]
        setup_only: bool,
    },

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
}

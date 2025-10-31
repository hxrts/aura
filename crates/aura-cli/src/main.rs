//! Capability-driven CLI for Aura
//!
//! Command-line interface for the Aura threshold identity platform.
//! Provides tools for account management, key derivation, and protocol testing.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
// Temporarily disabled - not needed without coordination
// use std::sync::Arc;

mod commands;
mod config;

// Temporarily disabled - requires coordination crate
// use aura_protocol::{production::ConsoleLogSink, LogSink};
use commands::{
    // Temporarily disabled - requires agent crate
    // authz::{handle_authz_command, AuthzCommand},
    common,
    frost::{self, FrostCommand},
    init,
    // network::{handle_network_command, NetworkCommand},
    node::{handle_node_command, NodeCommand},
    scenarios::{handle_scenarios_command, ScenariosArgs},
    status,
    storage::{handle_storage_command, StorageCommand},
    threshold::{handle_threshold_command, ThresholdCommand},
};

#[derive(Parser)]
#[command(name = "aura")]
#[command(about = "Aura - Capability-Based Identity and Storage Platform", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Config file path
    #[arg(short, long, global = true, default_value = ".aura/config.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new account with threshold capabilities
    Init {
        /// Number of participants
        #[arg(short = 'n', long, default_value = "3")]
        participants: u16,

        /// Threshold (M in M-of-N)
        #[arg(short = 't', long, default_value = "2")]
        threshold: u16,

        /// Output directory for configuration
        #[arg(short, long, default_value = ".aura")]
        output: String,
    },

    /// Show account status
    Status,

    /// Scenario management and execution
    Scenarios(ScenariosArgs),
    /// Start an Aura node with optional dev console
    Node(NodeCommand),
    /// Test threshold signature operations
    Threshold(ThresholdCommand),

    /// FROST threshold signature operations
    Frost(FrostCommand),
    //
    // /// Authorization commands - permission management (what you can do)
    // #[command(subcommand)]
    // Authz(AuthzCommand),
    //
    /// Storage operations with capability protection
    #[command(subcommand)]
    Storage(StorageCommand),
    //
    // Network and CGKA operations (disabled - requires API refactoring)
    // #[command(subcommand)]
    // Network(NetworkCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize unified logging system
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(log_level).init();

    // Temporarily disabled - requires coordination crate
    // Create console log sink for Aura logging
    // let _log_sink: Arc<dyn LogSink> = Arc::new(ConsoleLogSink::new());

    match cli.command {
        Commands::Init {
            participants,
            threshold,
            output,
        } => {
            init::run(participants, threshold, &output).await?;
        }

        Commands::Status => {
            let _config = common::load_config(&cli.config).await?;
            status::show_status(&cli.config.to_string_lossy()).await?;
        }

        Commands::Scenarios(args) => {
            handle_scenarios_command(args)?;
        } // Temporarily disabled - requires agent crate
        Commands::Node(cmd) => {
            let config = common::load_config(&cli.config).await?;
            handle_node_command(cmd, &config).await?;
        }

        Commands::Threshold(cmd) => {
            handle_threshold_command(cmd).await?;
        }

        Commands::Frost(cmd) => {
            frost::run(cmd).await?;
        }
        //
        // Commands::Authz(cmd) => {
        //     let config = common::load_config(&cli.config).await?;
        //     handle_authz_command(cmd, &config).await?;
        // }
        //
        Commands::Storage(cmd) => {
            let config = common::load_config(&cli.config).await?;
            handle_storage_command(cmd, &config).await?;
        } // //
          // Commands::Network(cmd) => {
          //     let config = common::load_config(&cli.config).await?;
          //     handle_network_command(cmd, &config).await?;
          // }
    }

    Ok(())
}

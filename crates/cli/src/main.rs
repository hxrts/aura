//! Capability-driven CLI for Aura
//!
//! Command-line interface for the Aura threshold identity platform.
//! Provides tools for account management, key derivation, and protocol testing.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
mod commands;

use config::Config;
use commands::{
    capability::{CapabilityCommand, handle_capability_command},
    storage::{StorageCommand, handle_storage_command},
    network::{NetworkCommand, handle_network_command},
    init,
    status,
    dkd,
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

    /// Test key derivation
    TestDkd {
        /// App ID
        #[arg(short, long)]
        app_id: String,

        /// Context label
        #[arg(short, long)]
        context: String,
    },

    /// Capability management commands
    #[command(subcommand)]
    Capability(CapabilityCommand),

    /// Storage operations with capability protection
    #[command(subcommand)]
    Storage(StorageCommand),

    /// Network and CGKA operations
    #[command(subcommand)]
    Network(NetworkCommand),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(log_level).init();

    match cli.command {
        Commands::Init {
            participants,
            threshold,
            output,
        } => {
            init::run(participants, threshold, &output).await?;
        }
        
        Commands::Status => {
            let _config = Config::load(&cli.config).await?;
            status::show_status(&cli.config.to_string_lossy()).await?;
        }
        
        Commands::TestDkd { app_id, context } => {
            dkd::test_dkd(&cli.config.to_string_lossy(), &app_id, &context).await?;
        }
        
        Commands::Capability(cmd) => {
            let config = Config::load(&cli.config).await?;
            handle_capability_command(cmd, &config).await?;
        }
        
        Commands::Storage(cmd) => {
            let config = Config::load(&cli.config).await?;
            handle_storage_command(cmd, &config).await?;
        }
        
        Commands::Network(cmd) => {
            let config = Config::load(&cli.config).await?;
            handle_network_command(cmd, &config).await?;
        }
    }

    Ok(())
}
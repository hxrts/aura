//! Capability-driven CLI for Aura
//!
//! Command-line interface for the Aura threshold identity platform.
//! Provides tools for account management, key derivation, and protocol testing.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

mod commands;
mod config;

use aura_coordination::{production::ConsoleLogSink, LogSink};
use commands::{
    authz::{handle_authz_command, AuthzCommand},
    init,
    network::{handle_network_command, NetworkCommand},
    status,
    storage::{handle_storage_command, StorageCommand},
};
use config::Config;

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

    /// Authorization commands - permission management (what you can do)
    #[command(subcommand)]
    Authz(AuthzCommand),

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

    // Initialize unified logging system
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(log_level).init();

    // Create console log sink for Aura logging
    let _log_sink: Arc<dyn LogSink> = Arc::new(ConsoleLogSink::new());

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

        Commands::Authz(cmd) => {
            let config = Config::load(&cli.config).await?;
            handle_authz_command(cmd, &config).await?;
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

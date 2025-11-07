//! Aura CLI Main Entry Point
//!
//! Command-line interface for the Aura threshold identity platform.
//! Uses the unified effect system for all operations.

use anyhow::Result;
use aura_protocol::{AuraEffectSystem, ConsoleEffects};
use aura_types::identifiers::DeviceId;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use aura_cli::{CliHandler, ScenarioAction};

#[derive(Parser)]
#[command(name = "aura")]
#[command(about = "Aura - Threshold Identity and Storage Platform", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Config file path
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new threshold account
    Init {
        /// Number of devices
        #[arg(short = 'n', long)]
        num_devices: u32,
        
        /// Threshold (minimum devices needed)
        #[arg(short = 't', long)]
        threshold: u32,
        
        /// Output directory
        #[arg(short = 'o', long)]
        output: PathBuf,
    },

    /// Show account status
    Status {
        /// Config file path
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },

    /// Run node/agent daemon
    Node {
        /// Port to listen on
        #[arg(long)]
        port: Option<u16>,
        
        /// Run as daemon
        #[arg(long)]
        daemon: bool,
        
        /// Config file path
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },

    /// Perform threshold operations
    Threshold {
        /// Comma-separated list of config files
        #[arg(long)]
        configs: String,
        
        /// Threshold number
        #[arg(long)]
        threshold: u32,
        
        /// Operation mode
        #[arg(long)]
        mode: String,
    },

    /// Scenario management
    Scenarios {
        #[command(subcommand)]
        action: ScenarioAction,
    },

    /// Test distributed key derivation
    TestDkd {
        /// Application ID
        #[arg(long)]
        app_id: String,
        
        /// Derivation context
        #[arg(long)]
        context: String,
        
        /// Config file path
        #[arg(short = 'f', long)]
        file: PathBuf,
    },

    /// Show version information
    Version,
}

        /// Scenarios directory

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create CLI device ID
    let device_id = DeviceId::new();
    
    // Initialize effect system based on environment
    let effect_system = if std::env::var("AURA_CLI_TEST").is_ok() {
        AuraEffectSystem::for_testing(device_id)
    } else {
        AuraEffectSystem::for_production(device_id)
    };

    // Initialize logging through effects
    let log_level = if cli.verbose { "debug" } else { "info" };
    effect_system.log_info(&format!("Initializing Aura CLI with log level: {}", log_level), &[]);

    // Create CLI handler
    let cli_handler = CliHandler::new(effect_system);

    // Execute command through effect system
    match &cli.command {
        Commands::Init { num_devices, threshold, output } => {
            cli_handler.handle_init(*num_devices, *threshold, output).await
        }
        Commands::Status { config } => {
            let config_path = resolve_config_path(config, &cli.config, &cli_handler).await?;
            cli_handler.handle_status(&config_path).await
        }
        Commands::Node { port, daemon, config } => {
            let config_path = resolve_config_path(config, &cli.config, &cli_handler).await?;
            cli_handler.handle_node(port.unwrap_or(58835), *daemon, &config_path).await
        }
        Commands::Threshold { configs, threshold, mode } => {
            cli_handler.handle_threshold(configs, *threshold, mode).await
        }
        Commands::Scenarios { action } => {
            cli_handler.handle_scenarios(action).await
        }
        Commands::TestDkd { app_id, context, file } => {
            cli_handler.handle_test_dkd(app_id, context, file).await
        }
        Commands::Version => {
            cli_handler.handle_version().await
        }
    }
}

async fn resolve_config_path(
    cmd_config: &Option<PathBuf>, 
    global_config: &Option<PathBuf>, 
    cli_handler: &CliHandler
) -> Result<PathBuf> {
    if let Some(config) = cmd_config {
        return Ok(config.clone());
    }
    if let Some(config) = global_config {
        return Ok(config.clone());
    }
    
    cli_handler.log_error("No config file specified. Use -c or --config to specify a config file.").await;
    anyhow::bail!("No config file specified")
}

// Legacy direct system functions removed - all operations now go through effect system

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::try_parse_from(&["aura", "version"]).unwrap();
        assert!(matches!(cli.command, Commands::Version));
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_init() {
        let cli = Cli::try_parse_from(&["aura", "init", "-n", "3", "-t", "2", "-o", "/tmp/test"]).unwrap();
        if let Commands::Init { num_devices, threshold, output } = cli.command {
            assert_eq!(num_devices, 3);
            assert_eq!(threshold, 2);
            assert_eq!(output, PathBuf::from("/tmp/test"));
        } else {
            panic!("Expected Init command");
        }
    }

    #[test]
    fn test_cli_scenarios() {
        let cli = Cli::try_parse_from(&["aura", "scenarios", "list", "--directory", "scenarios", "--detailed"]).unwrap();
        if let Commands::Scenarios { action } = cli.command {
            if let ScenarioAction::List { directory, detailed } = action {
                assert_eq!(directory, PathBuf::from("scenarios"));
                assert!(detailed);
            } else {
                panic!("Expected List scenario action");
            }
        } else {
            panic!("Expected Scenarios command");
        }
    }
}
//! Aura CLI Main Entry Point
//!
//! Command-line interface for the Aura threshold identity platform.
//! Uses the unified effect system for all operations.

use anyhow::Result;
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::{
    effects::ExecutionMode,
    identifiers::{AuthorityId, ContextId, DeviceId},
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use aura_cli::{
    AdminAction, AmpAction, AuthorityCommands, ChatCommands, CliHandler, ContextAction,
    InvitationAction, RecoveryAction, SnapshotAction,
};

#[cfg(feature = "development")]
use aura_cli::{DemoCommands, ScenarioAction};

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

    /// Scenario management (requires development feature)
    #[cfg(feature = "development")]
    Scenarios {
        #[command(subcommand)]
        action: ScenarioAction,
    },

    /// Interactive TUI demo with simulator (requires development feature)
    #[cfg(feature = "development")]
    Demo {
        #[command(subcommand)]
        command: DemoCommands,
    },

    /// Maintenance flows (snapshot, GC, OTA hooks)
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Admin maintenance (replace/fork)
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },

    /// Guardian recovery flows
    Recovery {
        #[command(subcommand)]
        action: RecoveryAction,
    },

    /// Device invitations
    Invite {
        #[command(subcommand)]
        action: InvitationAction,
    },

    /// Authority management (experimental)
    Authority {
        #[command(subcommand)]
        command: AuthorityCommands,
    },

    /// Show version information
    Version,

    /// Inspect relational contexts and rendezvous state
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },

    /// AMP channel inspection and bump flows (experimental)
    Amp {
        #[command(subcommand)]
        action: AmpAction,
    },

    /// Secure chat messaging
    Chat {
        #[command(subcommand)]
        command: ChatCommands,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create CLI device ID
    let device_id = DeviceId::new();
    let authority_id = AuthorityId::new();
    let context_id = ContextId::new();
    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

    // Initialize agent based on environment
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&effect_context)
        .await?;
    let effect_system = agent.runtime().effects().clone();

    // Initialize logging through effects
    let log_level = if cli.verbose { "debug" } else { "info" };
    println!("Initializing Aura CLI with log level: {}", log_level);

    // Create CLI handler
    let cli_handler = CliHandler::new(effect_system, device_id, effect_context);

    // Execute command through effect system
    match &cli.command {
        Commands::Init {
            num_devices,
            threshold,
            output,
        } => {
            cli_handler
                .handle_init(*num_devices, *threshold, output)
                .await
        }
        Commands::Status { config } => {
            let config_path = resolve_config_path(config, &cli.config, &cli_handler).await?;
            cli_handler.handle_status(&config_path).await
        }
        Commands::Node {
            port,
            daemon,
            config,
        } => {
            let config_path = resolve_config_path(config, &cli.config, &cli_handler).await?;
            cli_handler
                .handle_node(port.unwrap_or(58835), *daemon, &config_path)
                .await
        }
        Commands::Threshold {
            configs,
            threshold,
            mode,
        } => {
            cli_handler
                .handle_threshold(configs, *threshold, mode)
                .await
        }
        #[cfg(feature = "development")]
        Commands::Scenarios { action } => cli_handler.handle_scenarios(action).await,
        #[cfg(feature = "development")]
        Commands::Demo { command } => cli_handler.handle_demo(command).await,
        Commands::Snapshot { action } => cli_handler.handle_snapshot(action).await,
        Commands::Admin { action } => cli_handler.handle_admin(action).await,
        Commands::Recovery { action } => cli_handler.handle_recovery(action).await,
        Commands::Invite { action } => cli_handler.handle_invitation(action).await,
        Commands::Authority { command } => cli_handler.handle_authority(command).await,
        Commands::Context { action } => cli_handler.handle_context(action).await,
        Commands::Amp { action } => cli_handler.handle_amp(action).await,
        Commands::Chat { command } => cli_handler.handle_chat(command).await,
        Commands::Version => cli_handler.handle_version().await,
    }
}

/// Resolve the configuration file path from command line arguments
async fn resolve_config_path(
    cmd_config: &Option<PathBuf>,
    global_config: &Option<PathBuf>,
    _cli_handler: &CliHandler,
) -> Result<PathBuf> {
    if let Some(config) = cmd_config {
        return Ok(config.clone());
    }
    if let Some(config) = global_config {
        return Ok(config.clone());
    }

    eprintln!("No config file specified. Use -c or --config to specify a config file.");
    anyhow::bail!("No config file specified")
}

// Legacy direct system functions removed - all operations now go through effect system

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::try_parse_from(["aura", "version"]).unwrap();
        assert!(matches!(cli.command, Commands::Version));
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_init() {
        let cli =
            Cli::try_parse_from(["aura", "init", "-n", "3", "-t", "2", "-o", "/tmp/test"]).unwrap();
        if let Commands::Init {
            num_devices,
            threshold,
            output,
        } = cli.command
        {
            assert_eq!(num_devices, 3);
            assert_eq!(threshold, 2);
            assert_eq!(output, PathBuf::from("/tmp/test"));
        } else {
            panic!("Expected Init command");
        }
    }

    #[test]
    #[cfg(feature = "development")]
    fn test_cli_scenarios() {
        let cli = Cli::try_parse_from([
            "aura",
            "scenarios",
            "list",
            "--directory",
            "scenarios",
            "--detailed",
        ])
        .unwrap();
        if let Commands::Scenarios { action } = cli.command {
            if let ScenarioAction::List {
                directory,
                detailed,
            } = action
            {
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

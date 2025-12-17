//! Aura Terminal Main Entry Point
//! Uses bpaf for CLI parsing and delegates execution to CLI handlers.

use anyhow::Result;
// Import app types from aura-app (pure layer)
use aura_app::{AppConfig, AppCore};
// Import agent types from aura-agent (runtime layer)
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::effects::ExecutionMode;
use aura_terminal::cli::commands::{cli_parser, Commands, GlobalArgs, ThresholdArgs};
use aura_terminal::ids;
use aura_terminal::{CliHandler, SyncAction};
use bpaf::{Args, Parser};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Print a friendly usage message when no command is provided
fn print_usage() {
    eprintln!(
        "usage: aura [-v] [-c CONFIG] COMMAND [OPTIONS]

commands:
    init        Initialize a new threshold account
    status      Show account status
    node        Run node/agent daemon
    tui         Interactive terminal user interface
    chat        Secure messaging
    sync        Journal synchronization
    recovery    Guardian recovery flows
    invite      Device invitations
    authority   Authority management
    context     Relational context inspection
    amp         AMP channel operations
    version     Show version information

run 'aura COMMAND --help' for command-specific options"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    // Check if no arguments were provided (just "aura" with no command)
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() == 1 {
        print_usage();
        std::process::exit(0);
    }

    // Parse arguments, showing usage on parse failure
    let args: GlobalArgs = match cli_parser().to_options().run_inner(Args::current_args()) {
        Ok(args) => args,
        Err(e) => {
            // Check if this is a help request (exit code 0)
            let exit_code = e.clone().exit_code();
            if exit_code == 0 {
                print!("{:?}", e);
                std::process::exit(0);
            }
            // For other errors, show our friendly usage
            print_usage();
            std::process::exit(1);
        }
    };
    let command = args.command;

    // Create CLI device ID and identifiers
    let device_id = ids::device_id("cli:main-device");
    let authority_id = ids::authority_id("cli:main-authority");
    let context_id = ids::context_id("cli:main-context");
    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

    // Initialize agent and create AppCore (unified backend)
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&effect_context)
        .await?;
    let agent = Arc::new(agent);

    // Create AppCore with runtime bridge (dependency inversion pattern)
    let config = AppConfig::default();
    let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())?;
    let app_core = Arc::new(RwLock::new(app_core));

    // Initialize logging through effects
    let log_level = if args.verbose { "debug" } else { "info" };
    println!("Initializing Aura CLI with log level: {}", log_level);

    // Create CLI handler with agent and AppCore
    let cli_handler = CliHandler::with_agent(app_core, agent, device_id, effect_context);

    // Execute command through effect system
    Ok(match command {
        Commands::Init(init) => {
            cli_handler
                .handle_init(init.num_devices, init.threshold, &init.output)
                .await?
        }
        Commands::Status(status) => {
            let config_path =
                resolve_config_path(status.config.as_ref(), args.config.as_ref(), &cli_handler)
                    .await?;
            cli_handler.handle_status(&config_path).await?
        }
        Commands::Node(node) => {
            let config_path =
                resolve_config_path(node.config.as_ref(), args.config.as_ref(), &cli_handler)
                    .await?;
            cli_handler
                .handle_node(node.port.unwrap_or(58835), node.daemon, &config_path)
                .await?
        }
        Commands::Threshold(ThresholdArgs {
            configs,
            threshold,
            mode,
        }) => {
            cli_handler
                .handle_threshold(&configs, threshold, &mode)
                .await?
        }
        #[cfg(feature = "development")]
        Commands::Scenarios { action } => cli_handler.handle_scenarios(&action).await?,
        #[cfg(feature = "development")]
        Commands::Demo { command } => cli_handler.handle_demo(&command).await?,
        Commands::Snapshot { action } => cli_handler.handle_snapshot(&action).await?,
        Commands::Admin { action } => cli_handler.handle_admin(&action).await?,
        Commands::Recovery { action } => cli_handler.handle_recovery(&action).await?,
        Commands::Invite { action } => cli_handler.handle_invitation(&action).await?,
        Commands::Authority { command } => cli_handler.handle_authority(&command).await?,
        Commands::Context { action } => cli_handler.handle_context(&action).await?,
        Commands::Amp { action } => cli_handler.handle_amp(&action).await?,
        Commands::Chat { command } => cli_handler.handle_chat(&command).await?,
        Commands::Sync { action } => {
            // Default to daemon mode if no subcommand specified
            let sync_action = action.unwrap_or(SyncAction::Daemon {
                interval: 60,
                max_concurrent: 5,
                peers: None,
                config: None,
            });
            cli_handler.handle_sync(&sync_action).await?
        }
        #[cfg(feature = "terminal")]
        Commands::Tui(args) => cli_handler.handle_tui(&args).await?,
        Commands::Version => cli_handler.handle_version().await?,
    })
}

/// Resolve the configuration file path from command line arguments
async fn resolve_config_path(
    cmd_config: Option<&PathBuf>,
    global_config: Option<&PathBuf>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpaf::Args;
    use cfg_if::cfg_if;

    #[test]
    fn test_cli_parsing() {
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&["--verbose", "version"]))
            .unwrap();
        assert!(matches!(args.command, Commands::Version));
        assert!(args.verbose);
    }

    #[test]
    fn test_cli_init() {
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&[
                "init",
                "--num-devices",
                "3",
                "--threshold",
                "2",
                "--output",
                "/tmp/test",
            ]))
            .unwrap();

        if let Commands::Init(init) = args.command {
            assert_eq!(init.num_devices, 3);
            assert_eq!(init.threshold, 2);
            assert_eq!(init.output, PathBuf::from("/tmp/test"));
        } else {
            panic!("Expected Init command");
        }
    }

    #[test]
    fn test_cli_sync_default() {
        // Test that `aura sync` parses with no subcommand (daemon mode default)
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&["sync"]))
            .unwrap();
        if let Commands::Sync { action } = args.command {
            assert!(action.is_none());
        } else {
            panic!("Expected Sync command");
        }
    }

    #[test]
    fn test_cli_sync_daemon() {
        // Test explicit daemon subcommand with options
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&[
                "sync",
                "daemon",
                "--interval",
                "30",
                "--max-concurrent",
                "3",
            ]))
            .unwrap();
        if let Commands::Sync {
            action:
                Some(SyncAction::Daemon {
                    interval,
                    max_concurrent,
                    ..
                }),
        } = args.command
        {
            assert_eq!(interval, 30);
            assert_eq!(max_concurrent, 3);
        } else {
            panic!("Expected Sync daemon command");
        }
    }

    #[test]
    fn test_cli_sync_once() {
        // Test one-shot sync mode
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&["sync", "once", "--peers", "peer1,peer2"]))
            .unwrap();
        if let Commands::Sync {
            action: Some(SyncAction::Once { peers, .. }),
        } = args.command
        {
            assert_eq!(peers, "peer1,peer2");
        } else {
            panic!("Expected Sync once command");
        }
    }

    cfg_if! {
        if #[cfg(feature = "development")] {
            use aura_terminal::ScenarioAction;

            #[test]
            fn test_cli_scenarios() {
                let args = cli_parser()
                    .to_options()
                    .run_inner(Args::from(&[
                        "scenarios",
                        "list",
                        "--directory",
                        "scenarios",
                        "--detailed",
                    ]))
                    .unwrap();
                if let Commands::Scenarios { action } = args.command {
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
    }
}

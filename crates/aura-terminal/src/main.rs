//! Aura Terminal Main Entry Point
//! Uses bpaf for CLI parsing and delegates execution to CLI handlers.

use aura_core::{AuraConformanceArtifactV1, AuraError, ConformanceSurfaceName};
// Import app types from aura-app (pure layer)
use aura_app::ui::prelude::*;
use aura_app::ui::types::{BootstrapEvent, BootstrapEventKind, BootstrapSurface};
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::{default_storage_path, AgentConfig};
use aura_agent::{AgentBuilder, BuildError, EffectContext};
use aura_core::effects::ExecutionMode;
use aura_terminal::cli::commands::{cli_parser, Commands, GlobalArgs, ReplayArgs, ThresholdArgs};
use aura_terminal::handlers::{tui::try_load_account_from_path, CliOutput};
use aura_terminal::ids;
use aura_terminal::{CliHandler, SyncAction};
use bpaf::{Args, Parser};
use std::path::PathBuf;
use std::sync::Arc;

const USAGE: &str = r#"usage: aura [-v] [-c CONFIG] COMMAND [OPTIONS]

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
    replay      Replay conformance/effect trace artifacts
    version     Show version information

run 'aura COMMAND --help' for command-specific options"#;

fn usage_output(to_stderr: bool) -> CliOutput {
    let mut out = CliOutput::new();
    for line in USAGE.lines() {
        if to_stderr {
            out.eprintln(line.to_string());
        } else {
            out.println(line.to_string());
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<(), AuraError> {
    // Check if no arguments were provided (just "aura" with no command)
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() == 1 {
        usage_output(false).render();
        std::process::exit(0);
    }

    // Parse arguments, showing usage on parse failure
    let args: GlobalArgs = match cli_parser().to_options().run_inner(Args::current_args()) {
        Ok(args) => args,
        Err(e) => {
            // Check if this is a help request (exit code 0)
            let exit_code = e.clone().exit_code();
            if exit_code == 0 {
                CliOutput::new().println(format!("{e:?}")).render();
                std::process::exit(0);
            }
            // For other errors, show our friendly usage
            usage_output(true).render();
            std::process::exit(1);
        }
    };
    let command = args.command;

    if let Commands::Replay(replay) = &command {
        handle_replay_command(replay).await?;
        return Ok(());
    }

    if let Commands::Version = &command {
        CliOutput::new()
            .println(format!("aura {}", env!("CARGO_PKG_VERSION")))
            .println(format!("Package: {}", env!("CARGO_PKG_NAME")))
            .println(format!("Description: {}", env!("CARGO_PKG_DESCRIPTION")))
            .println(format!(
                "Repository: {} {}",
                env!("CARGO_PKG_REPOSITORY"),
                env!("CARGO_PKG_VERSION")
            ))
            .render();
        return Ok(());
    }

    if let Commands::Tui(tui_args) = &command {
        aura_terminal::handlers::tui::handle_tui(tui_args)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?;
        return Ok(());
    }

    // Create CLI device ID. Authority/context must come from persisted bootstrap state.
    let device_id = ids::device_id("cli:main-device");
    let storage_base_path = derive_storage_base_path(&command, args.config.as_ref())
        .unwrap_or_else(default_storage_path);
    let loaded_account = try_load_account_from_path(&storage_base_path)
        .await
        .map_err(|e| AuraError::agent(format!("failed to load persisted account: {e}")))?;
    let (authority_id, context_id) = match loaded_account {
        aura_terminal::handlers::tui::AccountLoadResult::Loaded { authority, context } => {
            (authority, context)
        }
        aura_terminal::handlers::tui::AccountLoadResult::NotFound => {
            let bootstrap_event = BootstrapEvent::new(
                BootstrapSurface::Terminal,
                BootstrapEventKind::RuntimeBootstrapRequired,
            );
            CliOutput::new()
                .eprintln(bootstrap_event.to_string())
                .render();
            return Err(AuraError::agent(
                BuildError::BootstrapRequired {
                    preset: "terminal",
                    identity: "persisted_account_identity",
                }
                .to_string(),
            ));
        }
    };
    let effect_context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

    // Initialize agent using CLI preset (unified backend)
    let mut agent_config = AgentConfig::default();
    agent_config.storage.base_path = storage_base_path;
    let agent = AgentBuilder::cli()
        .with_config(agent_config)
        .authority(authority_id)
        .context(context_id)
        .testing_mode()
        .build()
        .await
        .map_err(|e| AuraError::agent(format!("{e}")))?;
    let agent = Arc::new(agent);

    // Create AppCore with runtime bridge (dependency inversion pattern)
    let config = AppConfig::default();
    let app_core = AppCore::with_runtime(config, agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::agent(format!("{e}")))?;
    let app_core = Arc::new(RwLock::new(app_core));

    // Initialize logging through effects
    let log_level = if args.verbose { "debug" } else { "info" };
    CliOutput::new()
        .println(format!("Initializing Aura CLI with log level: {log_level}"))
        .render();

    // Create CLI handler with agent and AppCore
    let cli_handler = CliHandler::with_agent(app_core, agent, device_id, effect_context);

    // Execute command through effect system
    match command {
        Commands::Init(init) => cli_handler
            .handle_init(init.num_devices, init.threshold, &init.output)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Status(status) => {
            let config_path =
                resolve_config_path(status.config.as_ref(), args.config.as_ref(), &cli_handler)
                    .map_err(|e| AuraError::agent(format!("{e}")))?;
            cli_handler
                .handle_status(&config_path)
                .await
                .map_err(|e| AuraError::agent(format!("{e}")))?;
        }
        Commands::Node(node) => {
            let config_path =
                resolve_config_path(node.config.as_ref(), args.config.as_ref(), &cli_handler)
                    .map_err(|e| AuraError::agent(format!("{e}")))?;
            cli_handler
                .handle_node(node.port.unwrap_or(58835), node.daemon, &config_path)
                .await
                .map_err(|e| AuraError::agent(format!("{e}")))?;
        }
        Commands::Threshold(ThresholdArgs {
            configs,
            threshold,
            mode,
        }) => cli_handler
            .handle_threshold(&configs, threshold, &mode)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        #[cfg(feature = "development")]
        Commands::Scenarios { action } => cli_handler
            .handle_scenarios(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{}", e)))?,
        #[cfg(feature = "development")]
        Commands::Demo { command } => cli_handler
            .handle_demo(&command)
            .await
            .map_err(|e| AuraError::agent(format!("{}", e)))?,
        Commands::Snapshot { action } => cli_handler
            .handle_snapshot(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Admin { action } => cli_handler
            .handle_admin(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Recovery { action } => cli_handler
            .handle_recovery(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Invite { action } => cli_handler
            .handle_invitation(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Authority { command } => cli_handler
            .handle_authority(&command)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Replay(_) => unreachable!("replay command is handled before runtime boot"),
        Commands::Context { action } => cli_handler
            .handle_context(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Amp { action } => cli_handler
            .handle_amp(&action)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Chat { command } => cli_handler
            .handle_chat(&command)
            .await
            .map_err(|e| AuraError::agent(format!("{e}")))?,
        Commands::Sync { action } => {
            // Default to daemon mode if no subcommand specified
            let sync_action = action.unwrap_or(SyncAction::Daemon {
                interval: 60,
                max_concurrent: 5,
                peers: None,
                config: None,
            });
            cli_handler
                .handle_sync(&sync_action)
                .await
                .map_err(|e| AuraError::agent(format!("{e}")))?;
        }
        #[cfg(feature = "terminal")]
        Commands::Tui(_) => unreachable!("tui command is handled before runtime boot"),
        Commands::Version => unreachable!("version command is handled before runtime boot"),
    }

    Ok(())
}

fn derive_storage_base_path(
    command: &Commands,
    global_config: Option<&PathBuf>,
) -> Option<PathBuf> {
    match command {
        Commands::Init(init) => Some(init.output.clone()),
        Commands::Status(status) => {
            resolve_config_path_simple(status.config.as_ref(), global_config)
                .and_then(base_from_config_path)
        }
        Commands::Node(node) => resolve_config_path_simple(node.config.as_ref(), global_config)
            .and_then(base_from_config_path),
        Commands::Threshold(ThresholdArgs { configs, .. }) => {
            first_config_path(configs).and_then(base_from_config_path)
        }
        _ => None,
    }
}

fn resolve_config_path_simple(
    cmd_config: Option<&PathBuf>,
    global_config: Option<&PathBuf>,
) -> Option<PathBuf> {
    if let Some(config) = cmd_config {
        return Some(config.clone());
    }
    global_config.cloned()
}

fn first_config_path(configs: &str) -> Option<PathBuf> {
    configs
        .split(',')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn base_from_config_path(config_path: PathBuf) -> Option<PathBuf> {
    let parent = config_path.parent()?;
    if parent.file_name().is_some_and(|name| name == "configs") {
        return parent.parent().map(|p| p.to_path_buf());
    }
    Some(parent.to_path_buf())
}

#[derive(Debug, Clone, Copy)]
enum ReplayEncoding {
    Json,
    Cbor,
}

fn parse_replay_encoding(args: &ReplayArgs) -> Result<ReplayEncoding, AuraError> {
    if let Some(raw) = args.encoding.as_deref() {
        return match raw.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(ReplayEncoding::Json),
            "cbor" => Ok(ReplayEncoding::Cbor),
            _ => Err(AuraError::invalid(
                "invalid replay encoding; expected json or cbor",
            )),
        };
    }

    match args
        .trace_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("cbor") => Ok(ReplayEncoding::Cbor),
        _ => Ok(ReplayEncoding::Json),
    }
}

async fn handle_replay_command(args: &ReplayArgs) -> Result<(), AuraError> {
    let encoding = parse_replay_encoding(args)?;
    let payload = tokio::fs::read(&args.trace_file)
        .await
        .map_err(|error| AuraError::invalid(format!("failed to read trace file: {error}")))?;

    let artifact: AuraConformanceArtifactV1 = match encoding {
        ReplayEncoding::Json => serde_json::from_slice(&payload)
            .map_err(|error| AuraError::invalid(format!("invalid JSON trace artifact: {error}")))?,
        ReplayEncoding::Cbor => serde_cbor::from_slice(&payload)
            .map_err(|error| AuraError::invalid(format!("invalid CBOR trace artifact: {error}")))?,
    };

    artifact.validate_required_surfaces().map_err(|error| {
        AuraError::invalid(format!(
            "trace artifact missing required conformance surfaces: {error}"
        ))
    })?;

    let mut recomputed = artifact.clone();
    recomputed.recompute_digests().map_err(|error| {
        AuraError::invalid(format!("failed to recompute conformance digests: {error}"))
    })?;

    if !artifact.step_hashes.is_empty() && artifact.step_hashes != recomputed.step_hashes {
        return Err(AuraError::invalid(
            "trace artifact step_hashes mismatch: replay divergence detected",
        ));
    }

    if artifact.run_digest_hex.is_some() && artifact.run_digest_hex != recomputed.run_digest_hex {
        return Err(AuraError::invalid(
            "trace artifact run_digest mismatch: replay divergence detected",
        ));
    }

    for (surface, payload) in &artifact.surfaces {
        let Some(expected) = payload.digest_hex.as_ref() else {
            continue;
        };
        let Some(actual) = recomputed
            .surfaces
            .get(surface)
            .and_then(|value| value.digest_hex.as_ref())
        else {
            return Err(AuraError::invalid(format!(
                "trace artifact missing recomputed digest for surface {surface:?}"
            )));
        };
        if expected != actual {
            return Err(AuraError::invalid(format!(
                "trace artifact surface digest mismatch for {surface:?}: expected={expected} actual={actual}"
            )));
        }
    }

    let mut output = CliOutput::new();
    output
        .println(format!(
            "Replay verification passed: {}",
            args.trace_file.display()
        ))
        .println(format!(
            "scenario={} target={} profile={}",
            artifact.metadata.scenario, artifact.metadata.target, artifact.metadata.profile
        ))
        .println(format!(
            "surfaces={} step_hash_sets={} run_digest_present={}",
            artifact.surfaces.len(),
            artifact.step_hashes.len(),
            artifact.run_digest_hex.is_some()
        ));

    if args.visualize {
        append_replay_visualization(&mut output, &artifact);
    }
    if args.step_through {
        append_replay_step_through(&mut output, &artifact);
    }
    output.render();

    Ok(())
}

fn append_replay_visualization(output: &mut CliOutput, artifact: &AuraConformanceArtifactV1) {
    output.println("Replay visualization:".to_string());
    for surface in ConformanceSurfaceName::REQUIRED {
        if let Some(payload) = artifact.surfaces.get(&surface) {
            output.println(format!(
                "  {surface:?}: entries={} digest={}",
                payload.entries.len(),
                payload.digest_hex.as_deref().unwrap_or("<none>")
            ));
        }
    }
}

fn append_replay_step_through(output: &mut CliOutput, artifact: &AuraConformanceArtifactV1) {
    output.println("Replay step-through:".to_string());
    for surface in ConformanceSurfaceName::REQUIRED {
        let Some(payload) = artifact.surfaces.get(&surface) else {
            continue;
        };
        output.println(format!("  {surface:?}:"));
        for (index, entry) in payload.entries.iter().enumerate() {
            let rendered =
                serde_json::to_string(entry).unwrap_or_else(|_| "<unserializable>".to_string());
            output.println(format!("    [{index}] {rendered}"));
        }
    }
}

/// Resolve the configuration file path from command line arguments
fn resolve_config_path(
    cmd_config: Option<&PathBuf>,
    global_config: Option<&PathBuf>,
    _cli_handler: &CliHandler,
) -> Result<PathBuf, AuraError> {
    if let Some(config) = cmd_config {
        return Ok(config.clone());
    }
    if let Some(config) = global_config {
        return Ok(config.clone());
    }

    Err(AuraError::invalid(
        "No config file specified. Use -c or --config to specify a config file.",
    ))
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

    #[test]
    fn test_cli_replay() {
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&[
                "replay",
                "--trace-file",
                "artifacts/conformance/run.json",
                "--encoding",
                "json",
            ]))
            .unwrap();
        if let Commands::Replay(replay) = args.command {
            assert_eq!(
                replay.trace_file,
                PathBuf::from("artifacts/conformance/run.json")
            );
            assert_eq!(replay.encoding.as_deref(), Some("json"));
            assert!(!replay.visualize);
            assert!(!replay.step_through);
        } else {
            panic!("Expected Replay command");
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

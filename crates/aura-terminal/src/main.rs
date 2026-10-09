//! Aura Terminal Main Entry Point
//! Uses bpaf for CLI parsing and delegates execution to CLI handlers.
//!
//! Every command ends in one outcome: its output (text, or one JSON document
//! under `--json`) and exit code 0, or a typed [`CommandError`] whose code
//! fixes the exit status (see `aura_terminal::command::ErrorCode`).

use aura_core::{AuraConformanceArtifactV1, AuraError, ConformanceSurfaceName};
// Import app types from aura-app (pure layer)
use aura_app::ui::prelude::*;
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::AgentConfig;
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::effects::ExecutionMode;
use aura_terminal::cli::commands::{cli_parser, Commands, GlobalArgs, ReplayArgs, ThresholdArgs};
use aura_terminal::command::{
    confirm, execute, with_timeout, CommandContext, CommandError, ErrorCode, Outcome, OutputMode,
    Request,
};
use aura_terminal::handlers::{tui::open_production_runtime, CliOutput};
use aura_terminal::ids;
use aura_terminal::rpc_socket;
use aura_terminal::CliHandler;
use bpaf::{Args, Parser};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = r#"usage: aura [-v] [--json] [--yes] [--timeout SECONDS] [--data-dir DIR] COMMAND [OPTIONS]

commands:
    init        Initialize a new threshold account
    status      Show account status
    rpc         JSON-lines requests on stdin/stdout, node online
    serve       Keep the node online without a client
    tui         Interactive terminal user interface
    chat        Secure messaging
    sync        Journal synchronization
    recovery    Guardian recovery flows
    invite      Contact, guardian and channel invitations
    authority   Authority management
    context     Relational context inspection
    amp         AMP channel operations
    replay      Replay conformance/effect trace artifacts
    version     Show version information

global options:
    --json              print one JSON document: {"ok":true,"result":..} or {"ok":false,"error":..}
    -y, --yes           confirm destructive commands (required without a terminal)
    --timeout SECONDS   fail with exit code 5 when the command takes longer

exit codes: 0 ok, 1 failed, 2 invalid input, 3 not found, 4 permission denied,
            5 timeout, 6 unavailable

run 'aura COMMAND --help' for command-specific options"#;

const TOKIO_WORKER_STACK_SIZE_BYTES: usize = 32 * 1024 * 1024;

fn main() {
    let code = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES)
        .build()
    {
        Ok(runtime) => runtime.block_on(async_main()),
        Err(error) => {
            eprintln!("error: build terminal runtime: {error}");
            1
        }
    };
    std::process::exit(code);
}

/// Parse, run and report one command; returns the process exit code.
async fn async_main() -> i32 {
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() == 1 {
        println!("{USAGE}");
        return 0;
    }
    let json_requested = raw_args.iter().any(|arg| arg == "--json");

    let args: GlobalArgs = match cli_parser().to_options().run_inner(Args::current_args()) {
        Ok(args) => args,
        Err(e) => {
            if e.clone().exit_code() == 0 {
                // --help / --version style requests
                println!("{}", e.unwrap_stdout());
                return 0;
            }
            let error = CommandError::invalid(e.unwrap_stderr());
            OutputMode::from_json_flag(json_requested).emit_failure(&error, false);
            return error.exit_code();
        }
    };
    let mode = OutputMode::from_json_flag(args.json);
    let verbose = args.verbose;
    match run(args).await {
        Ok(outcome) => {
            mode.emit_success(&outcome);
            0
        }
        Err(error) => {
            mode.emit_failure(&error, verbose);
            error.exit_code()
        }
    }
}

async fn run(args: GlobalArgs) -> Result<Outcome, CommandError> {
    let GlobalArgs {
        verbose,
        data_dir,
        json,
        yes,
        timeout,
        command,
    } = args;
    // `aura ota publish` names files; the request carries their contents.
    let command = match command {
        Commands::OtaPublish {
            manifest,
            artifacts,
        } => Commands::Run(ota_publish_request(&manifest, &artifacts)?),
        other => other,
    };

    // Commands that need no account or runtime.
    match &command {
        Commands::Replay(replay) => return Ok(handle_replay_command(replay).await?.into()),
        Commands::Version => return Ok(aura_terminal::handlers::version::version_output().into()),
        #[cfg(feature = "terminal")]
        Commands::Tui(tui_args) => {
            // The global `--data-dir` may capture the flag written after `tui`.
            let mut tui_args = tui_args.clone();
            if tui_args.data_dir.is_none() {
                tui_args.data_dir = data_dir
                    .as_ref()
                    .map(|dir| dir.to_string_lossy().into_owned());
            }
            aura_terminal::handlers::tui::handle_tui(&tui_args).await?;
            return Ok(CliOutput::new().into());
        }
        Commands::Init(init) if init.output.is_absolute() => {
            return Err(CommandError::invalid(
                "--output must be a relative path; init writes it under the data directory",
            ));
        }
        // Ask before opening the runtime, so a refusal costs nothing.
        Commands::Run(request) => {
            if let Some(prompt) = request.confirmation() {
                confirm(&prompt, yes)?;
            }
        }
        _ => {}
    }

    // Create CLI device ID. Authority/context must come from persisted bootstrap state.
    let device_id = ids::device_id("cli:main-device");
    // Explicit data dir, then config-derived path, then the TUI's default
    // location so subcommands find the account the TUI created.
    let storage_base_path = data_dir
        .clone()
        .or_else(|| derive_storage_base_path(&command))
        .unwrap_or_else(|| {
            aura_terminal::handlers::tui::resolve_storage_path(
                None,
                aura_terminal::handlers::tui::TuiMode::Production,
            )
        });
    init_tracing(verbose, json);
    if let Commands::AccountCreate { nickname } = &command {
        // The TUI's own creation path: runtime-free staging, then the first
        // production launch initializes the runtime account.
        if rpc_socket::call(
            &rpc_socket::socket_path(&storage_base_path),
            &Request::Status,
            None,
        )
        .await?
        .is_some()
        {
            return Err(CommandError::invalid(format!(
                "a node already runs the account at {}",
                storage_base_path.display()
            )));
        }
        aura_terminal::handlers::tui::create_new_account(&storage_base_path, nickname).await?;
        return run_account_command(Commands::Run(Request::Status), &storage_base_path, timeout)
            .await;
    }
    if matches!(command, Commands::Run(_) | Commands::Rpc | Commands::Serve) {
        return run_account_command(command, &storage_base_path, timeout).await;
    }

    let timeout = timeout.map(Duration::from_secs);
    if let Commands::SyncDaemon(args) = &command {
        // The sync daemon runs on the account's production runtime.
        let runtime = open_account(&storage_base_path).await?;
        let effect_context = EffectContext::new(
            runtime.authority,
            runtime.context,
            ExecutionMode::Production,
        );
        let cli_handler = CliHandler::with_agent(
            runtime.app_core.clone(),
            runtime.agent.clone(),
            device_id,
            effect_context,
        );
        return Ok(cli_handler
            .handle_sync_daemon(args)
            .await
            .map_err(CommandError::from)?
            .into());
    }

    // Offline device-config tools (`init`, `threshold`) need no account: their
    // effects run under an identity derived from their inputs.
    let seed = match &command {
        Commands::Init(init) => format!("cli:init:{}", init.output.display()),
        Commands::Threshold(ThresholdArgs { configs, .. }) => format!("cli:threshold:{configs}"),
        _ => "cli:offline".to_string(),
    };
    let (authority_id, context_id) = (ids::authority_id(&seed), ids::context_id(&seed));
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
    let app_core = AppCore::with_runtime(AppConfig::default(), agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::agent(format!("{e}")))?;
    let app_core = Arc::new(RwLock::new(app_core));

    let cli_handler = CliHandler::with_agent(app_core.clone(), agent, device_id, effect_context);
    with_timeout(&app_core, timeout, || async {
        dispatch(&cli_handler, command)
            .await
            .map_err(CommandError::from)
    })
    .await
    .map(Outcome::from)
}

/// Runtime console messages go to stdout in text mode; with --json, stdout
/// carries only the result document, so they go to stderr.
fn init_tracing(verbose: bool, json: bool) {
    let filter = if verbose {
        "debug".to_string()
    } else {
        "off,aura_effects::console=info,aura_agent::runtime::effects::system=info".to_string()
    };
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .without_time()
        .with_target(false)
        .with_level(false)
        .with_ansi(false);
    let _ = if json {
        subscriber.with_writer(std::io::stderr).try_init()
    } else {
        subscriber.with_writer(std::io::stdout).try_init()
    };
}

/// Open the account's production runtime under the profile's exclusive
/// lease, as the TUI does.
async fn open_account(
    base_path: &std::path::Path,
) -> Result<aura_terminal::handlers::tui::ProductionRuntime, CommandError> {
    open_production_runtime(base_path)
        .await
        .map_err(|e| {
            let mut error = CommandError::from(e);
            error.message = format!(
                "Could not open the account at {} ({}). If another process holds it, run `aura serve` or the TUI there and retry.",
                base_path.display(),
                error.detail.as_deref().unwrap_or(&error.message)
            );
            error
        })?
        .ok_or_else(|| {
            CommandError::not_found(format!(
                "No Aura account found at {}. Create one with `aura tui`, or pass --data-dir <dir> pointing at an existing account.",
                base_path.display()
            ))
        })
}

/// Account commands (`Run`, `rpc`, `serve`). A node already running on this
/// data directory (the TUI or `aura serve`) answers over its socket;
/// otherwise this process opens the account's production runtime under the
/// profile's exclusive lease, exactly as the TUI does.
/// Read `aura ota publish`'s manifest (JSON) and artifact files into the
/// self-contained request the command model and `aura rpc` share.
fn ota_publish_request(
    manifest: &std::path::Path,
    artifacts: &[PathBuf],
) -> Result<Request, CommandError> {
    use base64::Engine as _;
    let read = |path: &std::path::Path| {
        std::fs::read(path)
            .map_err(|e| CommandError::invalid(format!("cannot read {}: {e}", path.display())))
    };
    let manifest = serde_json::from_slice(&read(manifest)?)
        .map_err(|e| CommandError::invalid(format!("{} is not JSON: {e}", manifest.display())))?;
    let artifacts = artifacts
        .iter()
        .map(|path| read(path).map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)))
        .collect::<Result<_, _>>()?;
    Ok(Request::OtaPublish {
        manifest,
        artifacts,
    })
}

async fn run_account_command(
    command: Commands,
    base_path: &std::path::Path,
    timeout: Option<u64>,
) -> Result<Outcome, CommandError> {
    let socket = rpc_socket::socket_path(base_path);
    match &command {
        Commands::Run(request) => {
            if let Some(result) = rpc_socket::call(&socket, request, timeout).await? {
                return Ok(Outcome::from_response(&result?));
            }
        }
        Commands::Rpc => {
            let attached = rpc_socket::attach_stdio(&socket)
                .await
                .map_err(|e| CommandError::new(ErrorCode::Unavailable, format!("rpc: {e}")))?;
            if attached {
                return Ok(Outcome::quiet());
            }
        }
        _ => {}
    }

    let runtime = open_account(base_path).await?;
    let app_core = runtime.app_core.clone();
    let ctx = CommandContext::new(
        app_core.clone(),
        runtime.agent.runtime().effects(),
        runtime.authority,
    );
    match command {
        Commands::Run(request) => {
            let timeout = timeout.map(Duration::from_secs);
            let response = with_timeout(&app_core, timeout, || execute(&ctx, request)).await?;
            Ok(Outcome::from_response(&response))
        }
        Commands::Rpc => {
            let stdin = tokio::io::BufReader::new(tokio::io::stdin());
            aura_terminal::rpc::serve(&ctx, stdin, tokio::io::stdout())
                .await
                .map_err(|e| CommandError::new(ErrorCode::Unavailable, format!("rpc: {e}")))?;
            Ok(Outcome::quiet())
        }
        _ => {
            let hosted = rpc_socket::host(base_path)
                .await
                .map_err(|e| CommandError::new(ErrorCode::Unavailable, format!("serve: {e}")))?;
            eprintln!(
                "aura serve: {} online at {}; Ctrl+C to stop",
                runtime.authority,
                hosted.path().display()
            );
            let stop = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            rpc_socket::serve(&ctx, hosted, stop)
                .await
                .map_err(|e| CommandError::new(ErrorCode::Failed, format!("serve: {e}")))?;
            let mut summary = CliOutput::new();
            summary.kv("Stopped", runtime.authority.to_string());
            Ok(summary.into())
        }
    }
}

/// Execute an offline tool or long-running mode.
async fn dispatch(
    cli_handler: &CliHandler,
    command: Commands,
) -> aura_terminal::TerminalResult<CliOutput> {
    match command {
        Commands::Init(init) => {
            cli_handler
                .handle_init(init.num_devices, init.threshold, &init.output)
                .await
        }
        Commands::Threshold(ThresholdArgs {
            configs,
            threshold,
            mode,
            message,
            message_hex,
            signature,
        }) => {
            cli_handler
                .handle_threshold(
                    &configs,
                    threshold,
                    &mode,
                    message.as_deref(),
                    message_hex.as_deref(),
                    signature.as_deref(),
                )
                .await
        }
        #[cfg(feature = "development")]
        Commands::Scenarios { action } => cli_handler.handle_scenarios(&action).await,
        #[cfg(feature = "development")]
        Commands::Demo { command } => cli_handler.handle_demo(&command).await,
        Commands::Run(_)
        | Commands::Rpc
        | Commands::Serve
        | Commands::SyncDaemon(_)
        | Commands::AccountCreate { .. }
        | Commands::OtaPublish { .. }
        | Commands::Replay(_)
        | Commands::Version => Err(aura_terminal::TerminalError::Operation(
            "command reached the offline tool dispatch".into(),
        )),
        #[cfg(feature = "terminal")]
        Commands::Tui(_) => Err(aura_terminal::TerminalError::Operation(
            "tui command reached runtime dispatch".into(),
        )),
    }
}

fn derive_storage_base_path(command: &Commands) -> Option<PathBuf> {
    match command {
        Commands::Init(init) => Some(init.output.clone()),
        Commands::Threshold(ThresholdArgs { configs, .. }) => {
            first_config_path(configs).and_then(base_from_config_path)
        }
        _ => None,
    }
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

async fn handle_replay_command(args: &ReplayArgs) -> Result<CliOutput, AuraError> {
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

    Ok(output)
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
        assert!(!args.json && !args.yes && args.timeout.is_none());
    }

    #[test]
    fn global_scripting_flags_parse() {
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&[
                "--json",
                "--yes",
                "--timeout",
                "30",
                "version",
            ]))
            .unwrap();
        assert!(args.json && args.yes);
        assert_eq!(args.timeout, Some(30));
    }

    #[test]
    fn zero_timeout_is_rejected() {
        assert!(cli_parser()
            .to_options()
            .run_inner(Args::from(&["--timeout", "0", "version"]))
            .is_err());
    }

    fn parse(argv: &[&str]) -> Commands {
        cli_parser()
            .to_options()
            .run_inner(Args::from(argv))
            .unwrap_or_else(|e| panic!("{argv:?}: {e:?}"))
            .command
    }

    #[test]
    fn account_commands_parse_into_typed_requests() {
        use aura_terminal::command::Request;
        let cases: Vec<(&[&str], Request)> = vec![
            (&["status"], Request::Status),
            (
                &["chat", "send", "general", "hello there"],
                Request::ChatSend {
                    channel: "general".into(),
                    message: "hello there".into(),
                },
            ),
            (
                &["chat", "history", "--limit", "5", "#general"],
                Request::ChatHistory {
                    channel: "#general".into(),
                    limit: Some(5),
                    sender: None,
                },
            ),
            (
                &["invite", "accept", "--invitation-id", "inv-1"],
                Request::InviteAccept {
                    invitation_id: "inv-1".into(),
                },
            ),
            (
                &["sync", "once", "--peers", "a,b"],
                Request::SyncOnce {
                    peers: vec!["a".into(), "b".into()],
                },
            ),
            (
                &["recovery", "start", "--guardians", "g1,g2"],
                Request::RecoveryStart {
                    guardians: vec!["g1".into(), "g2".into()],
                    threshold: 2,
                },
            ),
        ];
        for (argv, expected) in cases {
            match parse(argv) {
                Commands::Run(request) => assert_eq!(request, expected, "{argv:?}"),
                other => panic!("{argv:?} parsed as {other:?}"),
            }
        }
    }

    #[test]
    fn removed_service_only_commands_are_rejected() {
        for argv in [
            &["chat", "edit", "general", "x"][..],
            &["chat", "delete", "general", "x"][..],
            &["chat", "leave", "general", "--force"][..],
            &["authority", "create"][..],
            &["context", "receipts", "--context", "c"][..],
            &["recovery", "approve", "--request-file", "r.json"][..],
        ] {
            assert!(
                cli_parser()
                    .to_options()
                    .run_inner(Args::from(argv))
                    .is_err(),
                "{argv:?} should not parse"
            );
        }
    }

    #[test]
    fn sync_defaults_to_the_daemon() {
        assert!(matches!(parse(&["sync"]), Commands::SyncDaemon(args) if args.interval == 60));
        assert!(matches!(
            parse(&["sync", "daemon", "--interval", "30", "--max-concurrent", "3"]),
            Commands::SyncDaemon(args) if args.interval == 30 && args.max_concurrent == 3
        ));
    }

    #[test]
    fn tui_data_dir_is_available_whichever_parser_takes_it() {
        let args = cli_parser()
            .to_options()
            .run_inner(Args::from(&["tui", "--data-dir", "/tmp/aura-x"]))
            .unwrap();
        let Commands::Tui(tui_args) = &args.command else {
            panic!("expected tui command");
        };
        let effective = tui_args.data_dir.clone().or_else(|| {
            args.data_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned())
        });
        assert_eq!(effective.as_deref(), Some("/tmp/aura-x"));
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

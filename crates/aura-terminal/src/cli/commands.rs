use bpaf::{construct, long, pure, short, Parser};
use std::path::PathBuf;

use crate::cli::{
    chat::chat_parser,
    init::{init_parser, InitArgs},
    requests::{
        access_parser, account_parser, admin_parser, amp_parser, authority_parser, contact_parser,
        context_parser, device_parser, friend_parser, guardians_parser, home_parser, invite_parser,
        moderation_parser, neighborhood_parser, notifications_parser, peer_parser, profile_parser,
        recovery_parser, rotation_parser, settings_parser, slash_parser, status_parser,
    },
    sync::{sync_parser, SyncDaemonArgs},
    tui::tui_parser,
};
use crate::command::Request;

#[cfg(feature = "development")]
use crate::cli::demo::demo_parser;
#[cfg(feature = "development")]
use crate::DemoCommands;
#[cfg(feature = "development")]
use crate::ScenarioAction;
#[cfg(feature = "terminal")]
use crate::TuiArgs;

/// Threshold command arguments for the terminal CLI surface.
#[derive(Debug, Clone)]
pub struct ThresholdArgs {
    pub configs: String,
    pub threshold: u32,
    pub mode: String,
    pub message: Option<String>,
    pub message_hex: Option<String>,
    pub signature: Option<String>,
}

/// Replay command arguments for conformance/effect trace debugging.
#[derive(Debug, Clone)]
pub struct ReplayArgs {
    pub trace_file: PathBuf,
    pub encoding: Option<String>,
    pub visualize: bool,
    pub step_through: bool,
}

/// Top-level CLI commands exposed to the terminal.
///
/// Account commands parse straight into a typed [`Request`] (`Run`) that the
/// shared command model executes through the app workflows; the remaining
/// variants are offline tools or long-running modes.
#[derive(Debug, Clone)]
pub enum Commands {
    /// A workflow-backed account command.
    Run(Request),
    /// `aura account create`: create an account where none exists.
    AccountCreate {
        nickname: String,
    },
    Init(InitArgs),
    /// `aura rpc`: JSON-lines requests on stdin/stdout.
    Rpc,
    /// `aura serve`: stay online without a client.
    Serve,
    Threshold(ThresholdArgs),
    SyncDaemon(SyncDaemonArgs),
    #[cfg(feature = "development")]
    Scenarios {
        action: ScenarioAction,
    },
    #[cfg(feature = "development")]
    Demo {
        command: DemoCommands,
    },
    Replay(ReplayArgs),
    Version,
    #[cfg(feature = "terminal")]
    Tui(TuiArgs),
}

#[derive(Debug, Clone)]
pub struct GlobalArgs {
    pub verbose: bool,
    /// Account data directory shared with the TUI (`aura tui --data-dir`).
    pub data_dir: Option<PathBuf>,
    /// Print one JSON document per command instead of text.
    pub json: bool,
    /// Confirm destructive commands without prompting.
    pub yes: bool,
    /// Fail with exit code 5 when the command takes longer (seconds).
    pub timeout: Option<u64>,
    pub command: Commands,
}

#[must_use]
pub fn cli_parser() -> impl Parser<GlobalArgs> {
    let verbose = short('v')
        .long("verbose")
        .help("Enable verbose logging")
        .switch();
    let data_dir = long("data-dir")
        .help("Account data directory (same as `aura tui --data-dir`)")
        .argument::<PathBuf>("DIR")
        .optional();
    let json = long("json")
        .help("Print the result (or error) as one JSON document on stdout")
        .switch();
    let yes = short('y')
        .long("yes")
        .help("Confirm destructive commands without prompting (required without a terminal)")
        .switch();
    let timeout = long("timeout")
        .help("Fail with exit code 5 if the command takes longer than SECONDS")
        .argument::<u64>("SECONDS")
        .guard(
            |seconds| *seconds > 0,
            "--timeout must be at least 1 second",
        )
        .optional();
    let command = commands_parser();
    construct!(GlobalArgs {
        verbose,
        data_dir,
        json,
        yes,
        timeout,
        command
    })
}

/// A workflow-backed command group.
fn request_command(
    name: &'static str,
    help: &'static str,
    parser: impl Parser<Request> + 'static,
) -> impl Parser<Commands> {
    parser
        .to_options()
        .command(name)
        .help(help)
        .map(Commands::Run)
}

fn commands_parser() -> impl Parser<Commands> {
    let status = request_command("status", "Show account status", status_parser());
    let snapshot = request_command(
        "snapshot",
        "Record a snapshot proposal",
        pure(Request::SnapshotPropose),
    );
    let admin = request_command("admin", "Replace the account admin", admin_parser());
    let recovery = request_command("recovery", "Guardian recovery flows", recovery_parser());
    let invite = request_command(
        "invite",
        "Contact, guardian and channel invitations",
        invite_parser(),
    );
    let authority = request_command(
        "authority",
        "Authorities known to this runtime",
        authority_parser(),
    );
    let context = request_command("context", "Inspect relational contexts", context_parser());
    let amp = request_command("amp", "AMP channel inspection and bump flows", amp_parser());
    let chat = request_command("chat", "Secure chat messaging", chat_parser());
    let contact = request_command("contact", "Contacts", contact_parser());
    let home = request_command("home", "Homes: create, invite, accept", home_parser());
    let slash = request_command("slash", "Run a chat slash command", slash_parser());
    let friend = request_command("friend", "Friend requests", friend_parser());
    let neighborhood = request_command("neighborhood", "Neighborhoods", neighborhood_parser());
    let moderation = request_command("mod", "Home moderation", moderation_parser());
    let access = request_command("access", "Home access levels", access_parser());
    let peer = request_command("peer", "Peers", peer_parser());
    let notifications = request_command(
        "notifications",
        "Pending friend requests, invitations and recovery requests",
        notifications_parser(),
    );
    let social = construct!([
        friend,
        neighborhood,
        moderation,
        access,
        peer,
        notifications
    ]);
    let profile = request_command("profile", "Profile", profile_parser());
    let settings = request_command("settings", "Account settings", settings_parser());
    let device = request_command("device", "Devices and signing threshold", device_parser());
    let guardians = request_command("guardians", "Guardians", guardians_parser());
    let rotation = request_command("rotation", "Key-rotation ceremonies", rotation_parser());
    let budget = request_command("budget", "Home storage budget", pure(Request::Budget));
    let account = account_command();
    let account = construct!([account, profile, settings, device, guardians, rotation, budget]);
    let base = construct!([
        init_command(),
        status,
        rpc_command(),
        serve_command(),
        threshold_command(),
        snapshot,
        admin,
        recovery,
        invite,
        authority,
        replay_command(),
        version_command(),
        context,
        amp,
        chat,
        contact,
        home,
        slash,
        social,
        account,
        sync_command(),
    ]);

    #[cfg(feature = "terminal")]
    let base = construct!([base, tui_command()]);

    #[cfg(feature = "development")]
    let base = construct!([base, scenarios_command(), demo_command()]);

    base
}

fn init_command() -> impl Parser<Commands> {
    init_parser()
        .to_options()
        .command("init")
        .help("Initialize threshold device configs (offline)")
        .map(Commands::Init)
}

/// `aura account create --nickname N` (runtime-free creation, then the
/// first production launch) and `aura account refresh`.
fn account_command() -> impl Parser<Commands> {
    let create = long("nickname")
        .help("Nickname for the new account")
        .argument::<String>("NICKNAME")
        .map(|nickname| Commands::AccountCreate { nickname })
        .to_options()
        .command("create")
        .help("Create an account in the data directory");
    let refresh = account_parser().map(Commands::Run);
    construct!([create, refresh])
        .to_options()
        .command("account")
        .help("Create or refresh the account")
}

fn rpc_command() -> impl Parser<Commands> {
    pure(Commands::Rpc)
        .to_options()
        .command("rpc")
        .help("Serve JSON-lines requests on stdin/stdout with the node online")
}

fn serve_command() -> impl Parser<Commands> {
    pure(Commands::Serve)
        .to_options()
        .command("serve")
        .help("Keep the node online without a client until Ctrl+C")
}

fn sync_command() -> impl Parser<Commands> {
    sync_parser()
        .to_options()
        .command("sync")
        .help("Journal synchronization (daemon by default)")
}

fn threshold_command() -> impl Parser<Commands> {
    let configs = long("configs")
        .help("Comma-separated list of config files")
        .argument::<String>("CONFIGS");
    let threshold = long("threshold")
        .help("Threshold number")
        .argument::<u32>("THRESHOLD");
    let mode = long("mode")
        .help("Operation mode")
        .argument::<String>("MODE");
    let message = long("message")
        .help("Verification message as UTF-8 text (verify mode)")
        .argument::<String>("MESSAGE")
        .optional();
    let message_hex = long("message-hex")
        .help("Verification message as hex bytes (verify mode)")
        .argument::<String>("HEX")
        .optional();
    let signature = long("signature")
        .help("Hex/base64 encoded threshold signature (verify mode)")
        .argument::<String>("SIGNATURE")
        .optional();

    construct!(ThresholdArgs {
        configs,
        threshold,
        mode,
        message,
        message_hex,
        signature
    })
    .to_options()
    .command("threshold")
    .help("Perform threshold operations")
    .map(Commands::Threshold)
}

#[cfg(feature = "development")]
fn scenarios_command() -> impl Parser<Commands> {
    scenarios_parser()
        .to_options()
        .command("scenarios")
        .help("Scenario management (development feature)")
        .map(|action| Commands::Scenarios { action })
}

#[cfg(feature = "development")]
fn demo_command() -> impl Parser<Commands> {
    demo_parser()
        .to_options()
        .command("demo")
        .help("Interactive demos (development feature)")
        .map(|command| Commands::Demo { command })
}

fn replay_command() -> impl Parser<Commands> {
    let trace_file = long("trace-file")
        .help("Path to a conformance/effect trace artifact (json|cbor)")
        .argument::<PathBuf>("FILE");
    let encoding = long("encoding")
        .help("Trace encoding override (json|cbor). Default inferred from extension.")
        .argument::<String>("ENCODING")
        .optional();
    let visualize = long("visualize")
        .help("Render per-surface replay visualization output")
        .switch();
    let step_through = long("step-through")
        .help("Render step-by-step entry output for replay debugging")
        .switch();

    construct!(ReplayArgs {
        trace_file,
        encoding,
        visualize,
        step_through
    })
    .to_options()
    .command("replay")
    .help("Replay and validate conformance/effect trace artifacts")
    .map(Commands::Replay)
}

fn version_command() -> impl Parser<Commands> {
    pure(Commands::Version)
        .to_options()
        .command("version")
        .help("Show version information")
}

#[cfg(feature = "terminal")]
fn tui_command() -> impl Parser<Commands> {
    tui_parser()
        .to_options()
        .command("tui")
        .help("Interactive terminal user interface")
        .map(Commands::Tui)
}

#[cfg(feature = "development")]
fn scenarios_parser() -> impl Parser<ScenarioAction> {
    let discover = {
        let root = long("root")
            .help("Root directory to search")
            .argument::<PathBuf>("DIR");
        let validate = long("validate")
            .help("Whether to validate discovered scenarios")
            .switch();
        construct!(ScenarioAction::Discover { root, validate })
            .to_options()
            .command("discover")
    };

    let list = {
        let directory = long("directory")
            .help("Directory containing scenarios")
            .argument::<PathBuf>("DIR");
        let detailed = long("detailed").help("Show detailed information").switch();
        construct!(ScenarioAction::List {
            directory,
            detailed
        })
        .to_options()
        .command("list")
    };

    let validate = {
        let directory = long("directory")
            .help("Directory containing scenarios")
            .argument::<PathBuf>("DIR");
        let strictness = long("strictness")
            .help("Validation strictness level")
            .argument::<String>("LEVEL")
            .optional();
        construct!(ScenarioAction::Validate {
            directory,
            strictness
        })
        .to_options()
        .command("validate")
    };

    let run = {
        let directory = long("directory")
            .help("Directory containing scenarios")
            .argument::<PathBuf>("DIR")
            .optional();
        let pattern = long("pattern")
            .help("Pattern to match scenario names")
            .argument::<String>("PATTERN")
            .optional();
        let parallel = long("parallel").help("Run scenarios in parallel").switch();
        let max_parallel = long("max-parallel")
            .help("Maximum number of parallel scenarios")
            .argument::<usize>("COUNT")
            .optional();
        let output_file = long("output-file")
            .help("Output file for results")
            .argument::<PathBuf>("FILE")
            .optional();
        let detailed_report = long("detailed-report")
            .help("Generate detailed report")
            .switch();
        construct!(ScenarioAction::Run {
            directory,
            pattern,
            parallel,
            max_parallel,
            output_file,
            detailed_report
        })
        .to_options()
        .command("run")
    };

    let report = {
        let input = long("input")
            .help("Input results file")
            .argument::<PathBuf>("INPUT");
        let output = long("output")
            .help("Output report file")
            .argument::<PathBuf>("OUTPUT");
        let format = long("format")
            .help("Report format (text, json, html)")
            .argument::<String>("FORMAT")
            .optional();
        let detailed = long("detailed")
            .help("Include detailed information")
            .switch();
        construct!(ScenarioAction::Report {
            input,
            output,
            format,
            detailed
        })
        .to_options()
        .command("report")
    };

    construct!([discover, list, validate, run, report])
}

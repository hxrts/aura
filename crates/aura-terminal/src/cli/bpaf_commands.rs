use bpaf::{command, construct, long, pure, short, Parser};
use std::path::PathBuf;

use crate::cli::{
    amp::amp_parser,
    authority::authority_parser,
    bpaf_init::{init_parser, InitArgs},
    bpaf_node::{node_parser, NodeArgs},
    bpaf_status::{status_parser, StatusArgs},
    chat::chat_parser,
    context::context_parser,
    sync::sync_action_parser,
    tui::tui_parser,
};
use crate::{
    AdminAction, AmpAction, AuthorityCommands, ChatCommands, ContextAction, InvitationAction,
    RecoveryAction, SnapshotAction, SyncAction,
};

#[cfg(feature = "development")]
use crate::ScenarioAction;
#[cfg(feature = "development")]
use crate::cli::demo::demo_parser;
#[cfg(feature = "development")]
use crate::DemoCommands;
#[cfg(feature = "terminal")]
use crate::TuiArgs;

/// Threshold command arguments (legacy CLI compatibility).
#[derive(Debug, Clone)]
pub struct ThresholdArgs {
    pub configs: String,
    pub threshold: u32,
    pub mode: String,
}

/// Top-level CLI commands exposed to the terminal.
#[derive(Debug, Clone)]
pub enum Commands {
    Init(InitArgs),
    Status(StatusArgs),
    Node(NodeArgs),
    Threshold(ThresholdArgs),
    #[cfg(feature = "development")]
    Scenarios {
        action: ScenarioAction,
    },
    #[cfg(feature = "development")]
    Demo {
        command: DemoCommands,
    },
    Snapshot {
        action: SnapshotAction,
    },
    Admin {
        action: AdminAction,
    },
    Recovery {
        action: RecoveryAction,
    },
    Invite {
        action: InvitationAction,
    },
    Authority {
        command: AuthorityCommands,
    },
    Version,
    Context {
        action: ContextAction,
    },
    Amp {
        action: AmpAction,
    },
    Chat {
        command: ChatCommands,
    },
    Sync {
        action: Option<SyncAction>,
    },
    #[cfg(feature = "terminal")]
    Tui(TuiArgs),
}

#[derive(Debug, Clone)]
pub struct GlobalArgs {
    pub verbose: bool,
    pub config: Option<PathBuf>,
    pub command: Commands,
}

pub fn cli_parser() -> impl Parser<GlobalArgs> {
    let verbose = short('v')
        .long("verbose")
        .help("Enable verbose logging")
        .switch();
    let config = long("config")
        .short('c')
        .help("Global config file")
        .argument::<PathBuf>("CONFIG")
        .optional();
    let command = commands_parser();
    construct!(GlobalArgs {
        verbose,
        config,
        command
    })
}

fn commands_parser() -> impl Parser<Commands> {
    let base = construct!([
        init_command(),
        status_command(),
        node_command(),
        threshold_command(),
        snapshot_command(),
        admin_command(),
        recovery_command(),
        invite_command(),
        authority_command(),
        version_command(),
        context_command(),
        amp_command(),
        chat_command(),
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
        .help("Initialize a new threshold account")
        .map(Commands::Init)
}

fn status_command() -> impl Parser<Commands> {
    status_parser()
        .to_options()
        .command("status")
        .help("Show account status")
        .map(Commands::Status)
}

fn node_command() -> impl Parser<Commands> {
    node_parser()
        .to_options()
        .command("node")
        .help("Run node/agent daemon")
        .map(Commands::Node)
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

    construct!(ThresholdArgs {
        configs,
        threshold,
        mode
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

fn snapshot_command() -> impl Parser<Commands> {
    pure(SnapshotAction::Propose)
        .to_options()
        .command("snapshot")
        .help("Snapshot maintenance flows")
        .map(|action| Commands::Snapshot { action })
}

fn admin_command() -> impl Parser<Commands> {
    let account = long("account")
        .help("Account identifier (UUID string)")
        .argument::<String>("ACCOUNT");
    let new_admin = long("new-admin")
        .help("Device ID of the new admin (UUID string)")
        .argument::<String>("DEVICE");
    let activation_epoch = long("activation-epoch")
        .help("Epoch when the new admin becomes authoritative")
        .argument::<u64>("EPOCH");

    construct!(AdminAction::Replace {
        account,
        new_admin,
        activation_epoch
    })
    .to_options()
    .command("admin")
    .help("Admin maintenance")
    .map(|action| Commands::Admin { action })
}

fn recovery_command() -> impl Parser<Commands> {
    let start = {
        let account = long("account")
            .help("Account identifier to recover")
            .argument::<String>("ACCOUNT");
        let guardians = long("guardians")
            .help("Comma separated guardian device IDs")
            .argument::<String>("GUARDIANS");
        let threshold = long("threshold")
            .help("Required guardian threshold (default 2)")
            .argument::<u32>("THRESHOLD")
            .fallback(2);
        let priority = long("priority")
            .help("Recovery priority (normal|urgent|emergency)")
            .argument::<String>("PRIORITY")
            .fallback("normal".to_string());
        let dispute_hours = long("dispute-hours")
            .help("Dispute window in hours (default 48)")
            .argument::<u64>("HOURS")
            .fallback(48);
        let justification = long("justification")
            .help("Optional human readable justification")
            .argument::<String>("TEXT")
            .optional();
        construct!(RecoveryAction::Start {
            account,
            guardians,
            threshold,
            priority,
            dispute_hours,
            justification
        })
        .to_options()
        .command("start")
    };

    let approve = {
        let request_file = long("request-file")
            .help("Path to a serialized recovery request (JSON)")
            .argument::<PathBuf>("FILE");
        construct!(RecoveryAction::Approve { request_file })
            .to_options()
            .command("approve")
    };

    let status = pure(RecoveryAction::Status).to_options().command("status");

    let dispute = {
        let evidence = long("evidence")
            .help("Evidence identifier returned by 'aura recovery start'")
            .argument::<String>("EVIDENCE");
        let reason = long("reason")
            .help("Human readable reason included in the dispute log")
            .argument::<String>("REASON");
        construct!(RecoveryAction::Dispute { evidence, reason })
            .to_options()
            .command("dispute")
    };

    command(
        "recovery",
        construct!([start, approve, status, dispute]).to_options(),
    )
    .help("Guardian recovery flows")
    .map(|action| Commands::Recovery { action })
}

fn invite_command() -> impl Parser<Commands> {
    let create = {
        let account = long("account")
            .help("Account identifier")
            .argument::<String>("ACCOUNT");
        let invitee = long("invitee")
            .help("Device ID of the invitee")
            .argument::<String>("INVITEE");
        let role = long("role")
            .help("Role granted to the invitee")
            .argument::<String>("ROLE")
            .fallback("device".to_string());
        let ttl = long("ttl")
            .help("Optional TTL in seconds")
            .argument::<u64>("SECONDS")
            .optional();
        construct!(InvitationAction::Create {
            account,
            invitee,
            role,
            ttl
        })
        .to_options()
        .command("create")
    };

    let accept = {
        let envelope = long("envelope")
            .help("Path to the invitation envelope JSON file")
            .argument::<PathBuf>("FILE");
        construct!(InvitationAction::Accept { envelope })
            .to_options()
            .command("accept")
    };

    command("invite", construct!([create, accept]).to_options())
        .help("Device invitations")
        .map(|action| Commands::Invite { action })
}

fn authority_command() -> impl Parser<Commands> {
    authority_parser()
        .to_options()
        .command("authority")
        .help("Authority management")
        .map(|command| Commands::Authority { command })
}

fn version_command() -> impl Parser<Commands> {
    pure(Commands::Version)
        .to_options()
        .command("version")
        .help("Show version information")
}

fn context_command() -> impl Parser<Commands> {
    context_parser()
        .to_options()
        .command("context")
        .help("Inspect relational contexts and rendezvous state")
        .map(|action| Commands::Context { action })
}

fn amp_command() -> impl Parser<Commands> {
    amp_parser()
        .to_options()
        .command("amp")
        .help("AMP channel inspection and bump flows")
        .map(|action| Commands::Amp { action })
}

fn chat_command() -> impl Parser<Commands> {
    chat_parser()
        .to_options()
        .command("chat")
        .help("Secure chat messaging")
        .map(|command| Commands::Chat { command })
}

fn sync_command() -> impl Parser<Commands> {
    sync_action_parser()
        .optional()
        .to_options()
        .command("sync")
        .help("Journal synchronization (daemon by default)")
        .map(|action| Commands::Sync { action })
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

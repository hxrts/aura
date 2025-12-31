//! Context-level CLI commands for rendezvous and flow-budget inspection.

use bpaf::{construct, long, Parser};
use std::path::PathBuf;

/// Context debugging commands.
#[derive(Debug, Clone)]
pub enum ContextAction {
    /// Inspect rendezvous envelopes and channel health for a context.
    Inspect {
        context: String,
        state_file: PathBuf,
    },
    /// Show receipts and flow budget headroom for a context.
    Receipts {
        context: String,
        state_file: PathBuf,
        detailed: bool,
    },
}

fn inspect_command() -> impl Parser<ContextAction> {
    let context = long("context")
        .help("Context identifier (UUID/hex string)")
        .argument::<String>("CONTEXT");
    let state_file = long("state-file")
        .help("Path to a JSON state file exported by the runtime")
        .argument::<PathBuf>("FILE");
    construct!(ContextAction::Inspect {
        context,
        state_file
    })
    .to_options()
    .command("inspect")
    .help("Inspect rendezvous envelopes and channel health for a context")
}

fn receipts_command() -> impl Parser<ContextAction> {
    let context = long("context")
        .help("Context identifier (UUID/hex string)")
        .argument::<String>("CONTEXT");
    let state_file = long("state-file")
        .help("Path to a JSON state file exported by the runtime")
        .argument::<PathBuf>("FILE");
    let detailed = long("detailed")
        .help("Emit full receipt details instead of summary")
        .switch();
    construct!(ContextAction::Receipts {
        context,
        state_file,
        detailed
    })
    .to_options()
    .command("receipts")
    .help("Show receipts and flow budget headroom for a context")
}

#[must_use]
pub fn context_parser() -> impl Parser<ContextAction> {
    construct!([inspect_command(), receipts_command()])
}

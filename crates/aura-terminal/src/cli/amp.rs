//! AMP CLI commands for channel state inspection and epoch bumps.

use bpaf::{construct, long, Parser};

/// AMP commands for inspecting state and triggering bumps.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum AmpAction {
    /// Show channel epoch/windows for a context/channel.
    Inspect { context: String, channel: String },

    /// Propose a routine bump with reason.
    Bump {
        context: String,
        channel: String,
        /// Freeform reason (routine/emergency).
        reason: String,
    },

    /// Emit a checkpoint at the current generation.
    Checkpoint { context: String, channel: String },
}

fn inspect_command() -> impl Parser<AmpAction> {
    let context = long("context")
        .help("Context identifier")
        .argument::<String>("CONTEXT");
    let channel = long("channel")
        .help("Channel identifier")
        .argument::<String>("CHANNEL");
    construct!(AmpAction::Inspect { context, channel })
        .to_options()
        .command("inspect")
        .help("Show channel epoch/windows for a context/channel")
}

fn bump_command() -> impl Parser<AmpAction> {
    let context = long("context")
        .help("Context identifier")
        .argument::<String>("CONTEXT");
    let channel = long("channel")
        .help("Channel identifier")
        .argument::<String>("CHANNEL");
    let reason = long("reason")
        .help("Freeform reason (routine/emergency)")
        .argument::<String>("REASON");

    construct!(AmpAction::Bump {
        context,
        channel,
        reason
    })
    .to_options()
    .command("bump")
    .help("Propose a routine bump with reason")
}

fn checkpoint_command() -> impl Parser<AmpAction> {
    let context = long("context")
        .help("Context identifier")
        .argument::<String>("CONTEXT");
    let channel = long("channel")
        .help("Channel identifier")
        .argument::<String>("CHANNEL");

    construct!(AmpAction::Checkpoint { context, channel })
        .to_options()
        .command("checkpoint")
        .help("Emit a checkpoint at the current generation")
}

#[must_use]
pub fn amp_parser() -> impl Parser<AmpAction> {
    construct!([inspect_command(), bump_command(), checkpoint_command()])
}

//! AMP CLI commands for channel state inspection and epoch bumps.

use clap::Subcommand;

/// AMP commands for inspecting state and triggering bumps.
#[derive(Debug, Clone, Subcommand)]
#[allow(missing_docs)]
pub enum AmpAction {
    /// Show channel epoch/windows for a context/channel.
    Inspect {
        #[arg(long)]
        context: String,
        #[arg(long)]
        channel: String,
    },

    /// Propose a routine bump with reason.
    Bump {
        #[arg(long)]
        context: String,
        #[arg(long)]
        channel: String,
        /// Freeform reason (routine/emergency).
        #[arg(long)]
        reason: String,
    },

    /// Emit a checkpoint at the current generation.
    Checkpoint {
        #[arg(long)]
        context: String,
        #[arg(long)]
        channel: String,
    },
}

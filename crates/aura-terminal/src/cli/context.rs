//! Context-level CLI commands for rendezvous and flow-budget inspection.

use clap::Subcommand;
use std::path::PathBuf;

/// Context debugging commands.
#[derive(Debug, Clone, Subcommand)]
pub enum ContextAction {
    /// Inspect rendezvous envelopes and channel health for a context.
    Inspect {
        /// Context identifier (UUID/hex string).
        #[arg(long)]
        context: String,
        /// Path to a JSON state file exported by the runtime.
        #[arg(long)]
        state_file: PathBuf,
    },
    /// Show receipts and flow budget headroom for a context.
    Receipts {
        /// Context identifier (UUID/hex string).
        #[arg(long)]
        context: String,
        /// Path to a JSON state file exported by the runtime.
        #[arg(long)]
        state_file: PathBuf,
        /// Emit full receipt details instead of a condensed summary.
        #[arg(long)]
        detailed: bool,
    },
}

//! `aura sync`: journal synchronization.
//!
//! `status`, `once`, `add-peer` and `remove-peer` are workflow requests;
//! `daemon` (the default) keeps a sync service running in the foreground.

use crate::cli::commands::Commands;
use crate::cli::requests::sync_request_parser;
use bpaf::{construct, long, Parser};

/// Arguments of `aura sync daemon`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncDaemonArgs {
    /// Sync interval in seconds.
    pub interval: u64,
    /// Maximum concurrent sync sessions.
    pub max_concurrent: usize,
    /// Initial peers (comma-separated device IDs).
    pub peers: Option<String>,
}

impl Default for SyncDaemonArgs {
    fn default() -> Self {
        Self {
            interval: 60,
            max_concurrent: 5,
            peers: None,
        }
    }
}

fn daemon_parser() -> impl Parser<SyncDaemonArgs> {
    let interval = long("interval")
        .help("Sync interval in seconds (default: 60)")
        .argument::<u64>("SECONDS")
        .fallback(60);
    let max_concurrent = long("max-concurrent")
        .help("Maximum concurrent sync sessions (default: 5)")
        .argument::<usize>("COUNT")
        .fallback(5);
    let peers = long("peers")
        .help("Initial peers to sync with (comma-separated device IDs)")
        .argument::<String>("PEERS")
        .optional();
    construct!(SyncDaemonArgs {
        interval,
        max_concurrent,
        peers
    })
}

/// The `aura sync` subcommands; no subcommand runs the daemon.
#[must_use]
pub fn sync_parser() -> impl Parser<Commands> {
    let daemon = daemon_parser()
        .to_options()
        .command("daemon")
        .help("Start the sync daemon (default mode)")
        .map(Commands::SyncDaemon);
    let request = sync_request_parser().map(Commands::Run);
    construct!([daemon, request])
        .optional()
        .map(|command| command.unwrap_or_else(|| Commands::SyncDaemon(SyncDaemonArgs::default())))
}

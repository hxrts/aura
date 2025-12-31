//! Sync CLI Arguments - Sync daemon mode configuration
//!
//! Defines command-line arguments for journal synchronization operations.
//! The sync command runs in daemon mode by default, continuously synchronizing
//! with peers.

use bpaf::{construct, long, pure, Parser};
use std::path::PathBuf;

/// Sync subcommands for journal synchronization
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// Start the sync daemon (default mode)
    ///
    /// Runs in the foreground, synchronizing with discovered or configured peers.
    /// Use Ctrl+C to stop.
    Daemon {
        /// Sync interval in seconds (default: 60)
        interval: u64,

        /// Maximum concurrent sync sessions (default: 5)
        max_concurrent: usize,

        /// Initial peers to sync with (comma-separated device IDs)
        peers: Option<String>,

        /// Config file path
        config: Option<PathBuf>,
    },

    /// Perform a one-shot sync with specific peers
    Once {
        /// Peers to sync with (comma-separated device IDs)
        peers: String,

        /// Config file path
        config: Option<PathBuf>,
    },

    /// Show sync service status and metrics
    Status,

    /// Add a peer to the sync list
    AddPeer {
        /// Device ID of the peer to add
        peer: String,
    },

    /// Remove a peer from the sync list
    RemovePeer {
        /// Device ID of the peer to remove
        peer: String,
    },
}

fn daemon_command() -> impl Parser<SyncAction> {
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
    let config = long("config")
        .short('c')
        .help("Config file path")
        .argument::<PathBuf>("CONFIG")
        .optional();
    construct!(SyncAction::Daemon {
        interval,
        max_concurrent,
        peers,
        config
    })
    .to_options()
    .command("daemon")
    .help("Start the sync daemon (default mode)")
}

fn once_command() -> impl Parser<SyncAction> {
    let peers = long("peers")
        .help("Peers to sync with (comma-separated device IDs)")
        .argument::<String>("PEERS");
    let config = long("config")
        .short('c')
        .help("Config file path")
        .argument::<PathBuf>("CONFIG")
        .optional();
    construct!(SyncAction::Once { peers, config })
        .to_options()
        .command("once")
        .help("Perform a one-shot sync with specific peers")
}

fn status_command() -> impl Parser<SyncAction> {
    pure(SyncAction::Status)
        .to_options()
        .command("status")
        .help("Show sync service status and metrics")
}

fn add_peer_command() -> impl Parser<SyncAction> {
    let peer = long("peer")
        .help("Device ID of the peer to add")
        .argument::<String>("PEER");
    construct!(SyncAction::AddPeer { peer })
        .to_options()
        .command("add-peer")
        .help("Add a peer to the sync list")
}

fn remove_peer_command() -> impl Parser<SyncAction> {
    let peer = long("peer")
        .help("Device ID of the peer to remove")
        .argument::<String>("PEER");
    construct!(SyncAction::RemovePeer { peer })
        .to_options()
        .command("remove-peer")
        .help("Remove a peer from the sync list")
}

#[must_use]
pub fn sync_action_parser() -> impl Parser<SyncAction> {
    construct!([
        daemon_command(),
        once_command(),
        status_command(),
        add_peer_command(),
        remove_peer_command()
    ])
}

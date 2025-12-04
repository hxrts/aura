//! Sync CLI Arguments - Sync daemon mode configuration
//!
//! Defines command-line arguments for journal synchronization operations.
//! The sync command runs in daemon mode by default, continuously synchronizing
//! with peers.

use clap::Subcommand;
use std::path::PathBuf;

/// Sync subcommands for journal synchronization
#[derive(Debug, Clone, Subcommand)]
pub enum SyncAction {
    /// Start the sync daemon (default mode)
    ///
    /// Runs in the foreground, synchronizing with discovered or configured peers.
    /// Use Ctrl+C to stop.
    Daemon {
        /// Sync interval in seconds (default: 60)
        #[arg(long, default_value = "60")]
        interval: u64,

        /// Maximum concurrent sync sessions (default: 5)
        #[arg(long, default_value = "5")]
        max_concurrent: usize,

        /// Initial peers to sync with (comma-separated device IDs)
        #[arg(long)]
        peers: Option<String>,

        /// Config file path
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },

    /// Perform a one-shot sync with specific peers
    Once {
        /// Peers to sync with (comma-separated device IDs)
        #[arg(long)]
        peers: String,

        /// Config file path
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },

    /// Show sync service status and metrics
    Status,

    /// Add a peer to the sync list
    AddPeer {
        /// Device ID of the peer to add
        #[arg(long)]
        peer: String,
    },

    /// Remove a peer from the sync list
    RemovePeer {
        /// Device ID of the peer to remove
        #[arg(long)]
        peer: String,
    },
}

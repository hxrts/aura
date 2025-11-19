//! Authority-level CLI commands.

use aura_core::identifiers::AuthorityId;
use clap::Subcommand;

/// Authority management commands (definition only; handlers will be added later).
#[derive(Debug, Clone, Subcommand)]
pub enum AuthorityCommands {
    /// Create a new authority with optional threshold override.
    Create {
        /// Optional branch threshold (m-of-n) for the new authority.
        #[arg(long)]
        threshold: Option<u16>,
    },

    /// Display authority status (commitments, device counts, etc.).
    Status {
        /// Authority ID to inspect.
        #[arg(long)]
        authority_id: AuthorityId,
    },

    /// List all known authorities in the local runtime.
    List,

    /// Add a device public key to an authority.
    AddDevice {
        /// Target authority identifier.
        #[arg(long)]
        authority_id: AuthorityId,
        /// Hex/base64 encoded public key material.
        #[arg(long)]
        public_key: String,
    },
}

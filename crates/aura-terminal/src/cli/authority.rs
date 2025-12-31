//! Authority-level CLI commands.

use aura_core::identifiers::AuthorityId;
use bpaf::{construct, long, pure, Parser};

fn authority_id_arg(help: &'static str) -> impl Parser<AuthorityId> {
    long("authority-id")
        .help(help)
        .argument::<String>("AUTHORITY")
        .parse(|s: String| s.parse::<AuthorityId>().map_err(|e| e.to_string()))
}

/// Authority management commands (definition only; handlers will be added later).
#[derive(Debug, Clone)]
pub enum AuthorityCommands {
    /// Create a new authority with optional threshold override.
    Create {
        /// Optional branch threshold (m-of-n) for the new authority.
        threshold: Option<u16>,
    },

    /// Display authority status (commitments, device counts, etc.).
    Status {
        /// Authority ID to inspect.
        authority_id: AuthorityId,
    },

    /// List all known authorities in the local runtime.
    List,

    /// Add a device public key to an authority.
    AddDevice {
        /// Target authority identifier.
        authority_id: AuthorityId,
        /// Hex/base64 encoded public key material.
        public_key: String,
    },
}

fn create_command() -> impl Parser<AuthorityCommands> {
    let threshold = long("threshold")
        .help("Optional branch threshold (m-of-n)")
        .argument::<u16>("THRESHOLD")
        .optional();
    construct!(AuthorityCommands::Create { threshold })
        .to_options()
        .command("create")
        .help("Create a new authority with optional threshold override")
}

fn status_command() -> impl Parser<AuthorityCommands> {
    let authority_id = authority_id_arg("Authority ID to inspect");
    construct!(AuthorityCommands::Status { authority_id })
        .to_options()
        .command("status")
        .help("Display authority status")
}

fn list_command() -> impl Parser<AuthorityCommands> {
    pure(AuthorityCommands::List)
        .to_options()
        .command("list")
        .help("List known authorities")
}

fn add_device_command() -> impl Parser<AuthorityCommands> {
    let authority_id = authority_id_arg("Target authority identifier");
    let public_key = long("public-key")
        .help("Hex/base64 encoded public key material")
        .argument::<String>("KEY");
    construct!(AuthorityCommands::AddDevice {
        authority_id,
        public_key
    })
    .to_options()
    .command("add-device")
    .help("Add a device public key to an authority")
}

#[must_use]
pub fn authority_parser() -> impl Parser<AuthorityCommands> {
    construct!([
        create_command(),
        status_command(),
        list_command(),
        add_device_command()
    ])
}

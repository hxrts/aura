//! Chat CLI commands for secure group messaging
//!
//! This module defines the command-line interface for chat functionality,
//! including group creation, messaging, history retrieval, and member management.

use aura_core::identifiers::AuthorityId;
use bpaf::{construct, long, pure, short, Parser};
use uuid::Uuid;

fn authority_id_arg(
    long_flag: &'static str,
    short_flag: Option<char>,
    help: &'static str,
) -> impl Parser<AuthorityId> {
    let parser = long(long_flag).help(help);
    let parser = if let Some(s) = short_flag {
        parser.short(s)
    } else {
        parser
    };
    parser
        .argument::<String>("AUTHORITY")
        .parse(|s: String| s.parse::<AuthorityId>().map_err(|e| e.to_string()))
}

fn uuid_arg(
    long_flag: &'static str,
    short_flag: Option<char>,
    help: &'static str,
) -> impl Parser<Uuid> {
    let parser = long(long_flag).help(help);
    let parser = if let Some(s) = short_flag {
        parser.short(s)
    } else {
        parser
    };
    parser
        .argument::<String>("UUID")
        .parse(|s: String| Uuid::parse_str(&s).map_err(|e| e.to_string()))
}

/// Chat management commands for secure group messaging
#[derive(Debug, Clone)]
pub enum ChatCommands {
    /// Create a new chat group
    Create {
        /// Name for the chat group
        name: String,
        /// Description for the chat group (optional)
        description: Option<String>,
        /// Initial members to add to the group (space-separated authority IDs)
        members: Vec<AuthorityId>,
    },

    /// Send a message to a chat group
    Send {
        /// Group ID to send message to
        group_id: Uuid,
        /// Message content to send
        message: String,
        /// Optional message ID to reply to
        reply_to: Option<Uuid>,
    },

    /// Retrieve message history for a group
    History {
        /// Group ID to get history for
        group_id: Uuid,
        /// Number of messages to retrieve (default: 50)
        limit: usize,
        /// Only show messages before this timestamp (RFC3339 format)
        before: Option<String>,
        /// Filter by message type (text, system, edit, delete)
        message_type: Option<String>,
        /// Filter by sender authority ID
        sender: Option<AuthorityId>,
    },

    /// List all chat groups the user is a member of
    List,

    /// Show details for a specific chat group
    Show {
        /// Group ID to display details for
        group_id: Uuid,
        /// Show full member list with roles
        show_members: bool,
        /// Show group metadata
        show_metadata: bool,
    },

    /// Invite a user to a chat group
    Invite {
        /// Group ID to invite user to
        group_id: Uuid,
        /// Authority ID of user to invite
        authority_id: AuthorityId,
        /// Role to assign (member, admin, observer)
        role: String,
    },

    /// Leave a chat group
    Leave {
        /// Group ID to leave
        group_id: Uuid,
        /// Confirm leaving without prompt
        force: bool,
    },

    /// Remove a member from a chat group (admin only)
    Remove {
        /// Group ID to remove member from
        group_id: Uuid,
        /// Authority ID of member to remove
        member_id: AuthorityId,
        /// Confirm removal without prompt
        force: bool,
    },

    /// Update group metadata
    Update {
        /// Group ID to update
        group_id: Uuid,
        /// New group name
        name: Option<String>,
        /// New group description
        description: Option<String>,
        /// Set metadata key-value pairs (format: key=value)
        metadata: Vec<String>,
    },

    /// Search messages across groups
    Search {
        /// Search query string
        query: String,
        /// Limit search to specific group ID
        group_id: Option<Uuid>,
        /// Maximum number of results (default: 20)
        limit: usize,
        /// Filter by sender authority ID
        sender: Option<AuthorityId>,
    },

    /// Edit a previously sent message
    Edit {
        /// Group ID containing the message
        group_id: Uuid,
        /// Message ID to edit
        message_id: Uuid,
        /// New message content
        content: String,
    },

    /// Delete a message (soft delete with retraction)
    Delete {
        /// Group ID containing the message
        group_id: Uuid,
        /// Message ID to delete
        message_id: Uuid,
        /// Confirm deletion without prompt
        force: bool,
    },

    /// Export chat history to file
    Export {
        /// Group ID to export
        group_id: Uuid,
        /// Output file path
        output: String,
        /// Export format (json, csv, text)
        format: String,
        /// Include system messages in export
        include_system: bool,
    },
}

fn create_command() -> impl Parser<ChatCommands> {
    let name = short('n')
        .long("name")
        .help("Name for the chat group")
        .argument::<String>("NAME");
    let description = short('d')
        .long("description")
        .help("Description for the chat group")
        .argument::<String>("DESC")
        .optional();
    let members =
        authority_id_arg("members", Some('m'), "Initial members to add to the group").many();
    construct!(ChatCommands::Create {
        name,
        description,
        members
    })
    .to_options()
    .command("create")
    .help("Create a new chat group")
}

fn send_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to send message to");
    let message = short('m')
        .long("message")
        .help("Message content to send")
        .argument::<String>("MESSAGE");
    let reply_to = uuid_arg("reply-to", Some('r'), "Optional message ID to reply to").optional();
    construct!(ChatCommands::Send {
        group_id,
        message,
        reply_to
    })
    .to_options()
    .command("send")
    .help("Send a message to a chat group")
}

fn history_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to get history for");
    let limit = short('l')
        .long("limit")
        .help("Number of messages to retrieve (default: 50)")
        .argument::<usize>("LIMIT")
        .fallback(50);
    let before = long("before")
        .help("Only show messages before this timestamp (RFC3339)")
        .argument::<String>("TIMESTAMP")
        .optional();
    let message_type = long("message-type")
        .help("Filter by message type (text, system, edit, delete)")
        .argument::<String>("TYPE")
        .optional();
    let sender = authority_id_arg("sender", None, "Filter by sender authority ID").optional();
    construct!(ChatCommands::History {
        group_id,
        limit,
        before,
        message_type,
        sender
    })
    .to_options()
    .command("history")
    .help("Retrieve message history for a group")
}

fn list_command() -> impl Parser<ChatCommands> {
    pure(ChatCommands::List)
        .to_options()
        .command("list")
        .help("List all chat groups")
}

fn show_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to display details for");
    let show_members = long("show-members")
        .help("Show full member list with roles")
        .switch();
    let show_metadata = long("show-metadata").help("Show group metadata").switch();
    construct!(ChatCommands::Show {
        group_id,
        show_members,
        show_metadata
    })
    .to_options()
    .command("show")
    .help("Show details for a specific chat group")
}

fn invite_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to invite user to");
    let authority_id =
        authority_id_arg("authority-id", Some('a'), "Authority ID of user to invite");
    let role = short('r')
        .long("role")
        .help("Role to assign (member, admin, observer)")
        .argument::<String>("ROLE")
        .fallback("member".to_string());
    construct!(ChatCommands::Invite {
        group_id,
        authority_id,
        role
    })
    .to_options()
    .command("invite")
    .help("Invite a user to a chat group")
}

fn leave_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to leave");
    let force = long("force")
        .help("Confirm leaving without prompt")
        .switch();
    construct!(ChatCommands::Leave { group_id, force })
        .to_options()
        .command("leave")
        .help("Leave a chat group")
}

fn remove_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to remove member from");
    let member_id = authority_id_arg("member-id", Some('m'), "Authority ID of member to remove");
    let force = long("force")
        .help("Confirm removal without prompt")
        .switch();
    construct!(ChatCommands::Remove {
        group_id,
        member_id,
        force
    })
    .to_options()
    .command("remove")
    .help("Remove a member from a chat group (admin only)")
}

fn update_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to update");
    let name = long("name")
        .help("New group name")
        .argument::<String>("NAME")
        .optional();
    let description = long("description")
        .help("New group description")
        .argument::<String>("DESC")
        .optional();
    let metadata = long("metadata")
        .help("Set metadata key-value pairs (format: key=value)")
        .argument::<String>("PAIR")
        .many();
    construct!(ChatCommands::Update {
        group_id,
        name,
        description,
        metadata
    })
    .to_options()
    .command("update")
    .help("Update group metadata")
}

fn search_command() -> impl Parser<ChatCommands> {
    let query = short('q')
        .long("query")
        .help("Search query string")
        .argument::<String>("QUERY");
    let group_id = uuid_arg("group-id", Some('g'), "Limit search to specific group ID").optional();
    let limit = short('l')
        .long("limit")
        .help("Maximum number of results (default: 20)")
        .argument::<usize>("LIMIT")
        .fallback(20);
    let sender = authority_id_arg("sender", None, "Filter by sender authority ID").optional();
    construct!(ChatCommands::Search {
        query,
        group_id,
        limit,
        sender
    })
    .to_options()
    .command("search")
    .help("Search messages across groups")
}

fn edit_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID containing the message");
    let message_id = uuid_arg("message-id", Some('m'), "Message ID to edit");
    let content = short('c')
        .long("content")
        .help("New message content")
        .argument::<String>("CONTENT");
    construct!(ChatCommands::Edit {
        group_id,
        message_id,
        content
    })
    .to_options()
    .command("edit")
    .help("Edit a previously sent message")
}

fn delete_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID containing the message");
    let message_id = uuid_arg("message-id", Some('m'), "Message ID to delete");
    let force = long("force")
        .help("Confirm deletion without prompt")
        .switch();
    construct!(ChatCommands::Delete {
        group_id,
        message_id,
        force
    })
    .to_options()
    .command("delete")
    .help("Delete a message (soft delete with retraction)")
}

fn export_command() -> impl Parser<ChatCommands> {
    let group_id = uuid_arg("group-id", Some('g'), "Group ID to export");
    let output = short('o')
        .long("output")
        .help("Output file path")
        .argument::<String>("FILE");
    let format = short('f')
        .long("format")
        .help("Export format (json, csv, text)")
        .argument::<String>("FORMAT")
        .fallback("json".to_string());
    let include_system = long("include-system")
        .help("Include system messages in export")
        .switch();
    construct!(ChatCommands::Export {
        group_id,
        output,
        format,
        include_system
    })
    .to_options()
    .command("export")
    .help("Export chat history to file")
}

pub fn chat_parser() -> impl Parser<ChatCommands> {
    construct!([
        create_command(),
        send_command(),
        history_command(),
        list_command(),
        show_command(),
        invite_command(),
        leave_command(),
        remove_command(),
        update_command(),
        search_command(),
        edit_command(),
        delete_command(),
        export_command()
    ])
}

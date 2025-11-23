//! Chat CLI commands for secure group messaging
//!
//! This module defines the command-line interface for chat functionality,
//! including group creation, messaging, history retrieval, and member management.

use aura_core::identifiers::AuthorityId;
use clap::Subcommand;
use uuid::Uuid;

/// Chat management commands for secure group messaging
#[derive(Debug, Clone, Subcommand)]
pub enum ChatCommands {
    /// Create a new chat group
    Create {
        /// Name for the chat group
        #[arg(short, long)]
        name: String,

        /// Description for the chat group (optional)
        #[arg(short, long)]
        description: Option<String>,

        /// Initial members to add to the group (space-separated authority IDs)
        #[arg(short, long, value_delimiter = ' ')]
        members: Vec<AuthorityId>,
    },

    /// Send a message to a chat group
    Send {
        /// Group ID to send message to
        #[arg(short, long)]
        group_id: Uuid,

        /// Message content to send
        #[arg(short, long)]
        message: String,

        /// Optional message ID to reply to
        #[arg(short, long)]
        reply_to: Option<Uuid>,
    },

    /// Retrieve message history for a group
    History {
        /// Group ID to get history for
        #[arg(short, long)]
        group_id: Uuid,

        /// Number of messages to retrieve (default: 50)
        #[arg(short, long, default_value = "50")]
        limit: usize,

        /// Only show messages before this timestamp (RFC3339 format)
        #[arg(long)]
        before: Option<String>,

        /// Filter by message type (text, system, edit, delete)
        #[arg(long)]
        message_type: Option<String>,

        /// Filter by sender authority ID
        #[arg(long)]
        sender: Option<AuthorityId>,
    },

    /// List all chat groups the user is a member of
    List,

    /// Show details for a specific chat group
    Show {
        /// Group ID to display details for
        #[arg(short, long)]
        group_id: Uuid,

        /// Show full member list with roles
        #[arg(long)]
        show_members: bool,

        /// Show group metadata
        #[arg(long)]
        show_metadata: bool,
    },

    /// Invite a user to a chat group
    Invite {
        /// Group ID to invite user to
        #[arg(short, long)]
        group_id: Uuid,

        /// Authority ID of user to invite
        #[arg(short, long)]
        authority_id: AuthorityId,

        /// Role to assign (member, admin, observer)
        #[arg(short, long, default_value = "member")]
        role: String,
    },

    /// Leave a chat group
    Leave {
        /// Group ID to leave
        #[arg(short, long)]
        group_id: Uuid,

        /// Confirm leaving without prompt
        #[arg(long)]
        force: bool,
    },

    /// Remove a member from a chat group (admin only)
    Remove {
        /// Group ID to remove member from
        #[arg(short, long)]
        group_id: Uuid,

        /// Authority ID of member to remove
        #[arg(short, long)]
        member_id: AuthorityId,

        /// Confirm removal without prompt
        #[arg(long)]
        force: bool,
    },

    /// Update group metadata
    Update {
        /// Group ID to update
        #[arg(short, long)]
        group_id: Uuid,

        /// New group name
        #[arg(long)]
        name: Option<String>,

        /// New group description
        #[arg(long)]
        description: Option<String>,

        /// Set metadata key-value pairs (format: key=value)
        #[arg(long, value_delimiter = ' ')]
        metadata: Vec<String>,
    },

    /// Search messages across groups
    Search {
        /// Search query string
        #[arg(short, long)]
        query: String,

        /// Limit search to specific group ID
        #[arg(short, long)]
        group_id: Option<Uuid>,

        /// Maximum number of results (default: 20)
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Filter by sender authority ID
        #[arg(long)]
        sender: Option<AuthorityId>,
    },

    /// Edit a previously sent message
    Edit {
        /// Group ID containing the message
        #[arg(short, long)]
        group_id: Uuid,

        /// Message ID to edit
        #[arg(short, long)]
        message_id: Uuid,

        /// New message content
        #[arg(short, long)]
        content: String,
    },

    /// Delete a message (soft delete with tombstone)
    Delete {
        /// Group ID containing the message
        #[arg(short, long)]
        group_id: Uuid,

        /// Message ID to delete
        #[arg(short, long)]
        message_id: Uuid,

        /// Confirm deletion without prompt
        #[arg(long)]
        force: bool,
    },

    /// Export chat history to file
    Export {
        /// Group ID to export
        #[arg(short, long)]
        group_id: Uuid,

        /// Output file path
        #[arg(short, long)]
        output: String,

        /// Export format (json, csv, text)
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Include system messages in export
        #[arg(long)]
        include_system: bool,
    },
}

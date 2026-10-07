//! Typed command requests.
//!
//! One [`Request`] per command. The CLI parses its arguments into a
//! `Request`; `aura rpc` reads the same type from JSON
//! (`{"method":"chat_send","params":{...}}`). Identifiers stay strings here
//! and are parsed by the executor, so CLI and RPC reject bad input the same
//! way.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Role granted by `invite create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InviteRole {
    /// Become a contact.
    #[default]
    Contact,
    /// Become one of this account's guardians.
    Guardian,
    /// Join a channel (requires `channel`).
    Channel,
}

impl std::str::FromStr for InviteRole {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "contact" => Ok(Self::Contact),
            "guardian" => Ok(Self::Guardian),
            "channel" => Ok(Self::Channel),
            other => Err(format!(
                "unknown role {other}; expected contact, guardian or channel"
            )),
        }
    }
}

/// Format of `chat export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// A JSON array of messages.
    #[default]
    Json,
    /// CSV with a header row.
    Csv,
    /// One line per message.
    Text,
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "text" => Ok(Self::Text),
            other => Err(format!(
                "unknown export format {other}; expected json, csv or text"
            )),
        }
    }
}

fn default_search_limit() -> usize {
    20
}

/// A command against the account's runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// Account identity, threshold, devices and contacts.
    Status,
    /// Authorities known to this runtime.
    AuthorityList,
    /// This account's contacts.
    ContactList,

    /// Channels this account has joined.
    ChatList,
    /// One channel's details and members.
    ChatShow { channel: String },
    /// A channel's messages, oldest first.
    ChatHistory {
        channel: String,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        sender: Option<String>,
    },
    /// Send a message.
    ChatSend { channel: String, message: String },
    /// Create a channel.
    ChatCreate {
        name: String,
        #[serde(default)]
        topic: Option<String>,
        #[serde(default)]
        members: Vec<String>,
    },
    /// Invite an authority into a channel.
    ChatInvite { channel: String, authority: String },
    /// Leave a channel.
    ChatLeave { channel: String },
    /// Rename a channel or set its topic.
    ChatUpdate {
        channel: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        topic: Option<String>,
    },
    /// Find messages containing a text.
    ChatSearch {
        query: String,
        #[serde(default)]
        channel: Option<String>,
        #[serde(default)]
        sender: Option<String>,
        #[serde(default = "default_search_limit")]
        limit: usize,
    },
    /// A channel's history in a portable format.
    ChatExport {
        channel: String,
        #[serde(default)]
        format: ExportFormat,
    },

    /// Create an invitation and its shareable code.
    InviteCreate {
        invitee: String,
        #[serde(default)]
        role: InviteRole,
        #[serde(default)]
        channel: Option<String>,
        #[serde(default)]
        ttl_secs: Option<u64>,
    },
    /// Accept a received invitation.
    InviteAccept { invitation_id: String },
    /// Decline a received invitation.
    InviteDecline { invitation_id: String },
    /// Cancel an invitation this account sent.
    InviteCancel { invitation_id: String },
    /// Pending invitations.
    InviteList,
    /// The shareable code of an invitation.
    InviteExport { invitation_id: String },
    /// Import a shareable code so it can be accepted.
    InviteImport { code: String },

    /// Create a home.
    HomeCreate {
        #[serde(default)]
        name: Option<String>,
    },
    /// Invite an authority into the current home.
    HomeInvite { authority: String },
    /// Accept the pending home or channel invitation.
    HomeAccept,
    /// Run a chat slash command (`/kick bob`, `/topic ..`), as typed in the
    /// TUI's chat input, optionally in a given channel.
    Slash {
        command: String,
        #[serde(default)]
        channel: Option<String>,
    },

    /// Start guardian recovery.
    RecoveryStart {
        guardians: Vec<String>,
        threshold: u16,
    },
    /// Approve a recovery ceremony as a guardian.
    RecoveryApprove { ceremony_id: String },
    /// Dispute a recovery ceremony as a guardian.
    RecoveryDispute { ceremony_id: String, reason: String },
    /// Guardians and recovery progress.
    RecoveryStatus,

    /// Channels bound to a relational context.
    ContextInspect { context: String },

    /// Journal sync status.
    SyncStatus,
    /// Sync now: with the given peers, or with every peer.
    SyncOnce {
        #[serde(default)]
        peers: Vec<String>,
    },
    /// Track a peer as connected.
    PeerAdd { peer: String },
    /// Stop tracking a peer.
    PeerRemove { peer: String },

    /// AMP channel epoch state.
    AmpInspect { context: String, channel: String },
    /// Propose an AMP channel epoch bump.
    AmpBump { context: String, channel: String },
    /// Emit an AMP checkpoint at the current generation.
    AmpCheckpoint { context: String, channel: String },

    /// Record a snapshot proposal.
    SnapshotPropose,
    /// Record an admin replacement.
    AdminReplace {
        account: String,
        new_admin: String,
        activation_epoch: u64,
    },
}

impl Request {
    /// The confirmation prompt for a destructive request, if it is one.
    /// The CLI asks (or requires `--yes`); RPC requests are explicit.
    #[must_use]
    pub fn confirmation(&self) -> Option<String> {
        match self {
            Self::ChatLeave { channel } => Some(format!("Leave channel {channel}?")),
            Self::InviteCancel { invitation_id } => {
                Some(format!("Cancel invitation {invitation_id}?"))
            }
            Self::AdminReplace { new_admin, .. } => {
                Some(format!("Replace the account admin with {new_admin}?"))
            }
            _ => None,
        }
    }

    /// The wire name of this request (`method`).
    #[must_use]
    pub fn method(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value["method"].as_str().map(str::to_string))
            .unwrap_or_default()
    }
}

/// One example of every request kind. The exhaustive match below fails to
/// compile when a variant is added without an example, which keeps the
/// advertised method list (`aura rpc` hello line) complete.
#[must_use]
pub fn all_request_examples() -> Vec<Request> {
    let s = String::new;
    let examples = vec![
        Request::Status,
        Request::AuthorityList,
        Request::ContactList,
        Request::ChatList,
        Request::ChatShow { channel: s() },
        Request::ChatHistory {
            channel: s(),
            limit: None,
            sender: None,
        },
        Request::ChatSend {
            channel: s(),
            message: s(),
        },
        Request::ChatCreate {
            name: s(),
            topic: None,
            members: Vec::new(),
        },
        Request::ChatInvite {
            channel: s(),
            authority: s(),
        },
        Request::ChatLeave { channel: s() },
        Request::ChatUpdate {
            channel: s(),
            name: None,
            topic: None,
        },
        Request::ChatSearch {
            query: s(),
            channel: None,
            sender: None,
            limit: default_search_limit(),
        },
        Request::ChatExport {
            channel: s(),
            format: ExportFormat::Json,
        },
        Request::InviteCreate {
            invitee: s(),
            role: InviteRole::Contact,
            channel: None,
            ttl_secs: None,
        },
        Request::InviteAccept { invitation_id: s() },
        Request::InviteDecline { invitation_id: s() },
        Request::InviteCancel { invitation_id: s() },
        Request::InviteList,
        Request::InviteExport { invitation_id: s() },
        Request::InviteImport { code: s() },
        Request::HomeCreate { name: None },
        Request::HomeInvite { authority: s() },
        Request::HomeAccept,
        Request::Slash {
            command: s(),
            channel: None,
        },
        Request::RecoveryStart {
            guardians: Vec::new(),
            threshold: 2,
        },
        Request::RecoveryApprove { ceremony_id: s() },
        Request::RecoveryDispute {
            ceremony_id: s(),
            reason: s(),
        },
        Request::RecoveryStatus,
        Request::ContextInspect { context: s() },
        Request::SyncStatus,
        Request::SyncOnce { peers: Vec::new() },
        Request::PeerAdd { peer: s() },
        Request::PeerRemove { peer: s() },
        Request::AmpInspect {
            context: s(),
            channel: s(),
        },
        Request::AmpBump {
            context: s(),
            channel: s(),
        },
        Request::AmpCheckpoint {
            context: s(),
            channel: s(),
        },
        Request::SnapshotPropose,
        Request::AdminReplace {
            account: s(),
            new_admin: s(),
            activation_epoch: 0,
        },
    ];
    for example in &examples {
        match example {
            Request::Status
            | Request::AuthorityList
            | Request::ContactList
            | Request::ChatList
            | Request::ChatShow { .. }
            | Request::ChatHistory { .. }
            | Request::ChatSend { .. }
            | Request::ChatCreate { .. }
            | Request::ChatInvite { .. }
            | Request::ChatLeave { .. }
            | Request::ChatUpdate { .. }
            | Request::ChatSearch { .. }
            | Request::ChatExport { .. }
            | Request::InviteCreate { .. }
            | Request::InviteAccept { .. }
            | Request::InviteDecline { .. }
            | Request::InviteCancel { .. }
            | Request::InviteList
            | Request::InviteExport { .. }
            | Request::InviteImport { .. }
            | Request::HomeCreate { .. }
            | Request::HomeInvite { .. }
            | Request::HomeAccept
            | Request::Slash { .. }
            | Request::RecoveryStart { .. }
            | Request::RecoveryApprove { .. }
            | Request::RecoveryDispute { .. }
            | Request::RecoveryStatus
            | Request::ContextInspect { .. }
            | Request::SyncStatus
            | Request::SyncOnce { .. }
            | Request::PeerAdd { .. }
            | Request::PeerRemove { .. }
            | Request::AmpInspect { .. }
            | Request::AmpBump { .. }
            | Request::AmpCheckpoint { .. }
            | Request::SnapshotPropose
            | Request::AdminReplace { .. } => {}
        }
    }
    examples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_use_method_and_params_on_the_wire() {
        let request = Request::ChatSend {
            channel: "general".into(),
            message: "hi".into(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "chat_send");
        assert_eq!(json["params"]["channel"], "general");
        assert_eq!(serde_json::from_value::<Request>(json).unwrap(), request);
        assert_eq!(request.method(), "chat_send");
    }

    #[test]
    fn unit_requests_need_no_params_and_optional_params_default() {
        let status: Request = serde_json::from_str(r#"{"method":"status"}"#).unwrap();
        assert_eq!(status, Request::Status);
        let search: Request =
            serde_json::from_str(r#"{"method":"chat_search","params":{"query":"x"}}"#).unwrap();
        assert!(matches!(search, Request::ChatSearch { limit: 20, .. }));
    }
}

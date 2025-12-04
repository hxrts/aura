//! Query type definitions
//!
//! Each query type implements the `Query` trait for typed Datalog compilation.

use serde::{Deserialize, Serialize};

use super::{Binding, DatalogRule, Query};

// =============================================================================
// Query Types
// =============================================================================

/// Query for channels
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChannelsQuery {
    /// Filter by channel type
    pub channel_type: Option<String>,
    /// Include archived channels
    pub include_archived: bool,
}

impl Query for ChannelsQuery {
    type Result = Vec<crate::views::chat::Channel>;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        let mut body = vec!["channel($id, $name, $type, $topic, $archived)".to_string()];

        // Add channel_type filter if specified
        if let Some(ref channel_type) = self.channel_type {
            body.push(format!("$type = \"{}\"", channel_type));
        }

        // Filter out archived unless include_archived is true
        if !self.include_archived {
            body.push("$archived = false".to_string());
        }

        vec![DatalogRule {
            head: "channel($id, $name, $type, $topic, $archived)".to_string(),
            body,
        }]
    }

    fn parse_results(bindings: Vec<Vec<Binding>>) -> Self::Result {
        use crate::views::chat::{Channel, ChannelType};

        bindings
            .into_iter()
            .map(|row| {
                // Helper to find binding by name
                let find = |name: &str| -> String {
                    row.iter()
                        .find(|b| b.name == name)
                        .map(|b| b.value.clone())
                        .unwrap_or_default()
                };

                let channel_type = match find("type").as_str() {
                    "DirectMessage" => ChannelType::DirectMessage,
                    "Guardian" => ChannelType::Guardian,
                    _ => ChannelType::Block,
                };

                Channel {
                    id: find("id"),
                    name: find("name"),
                    topic: {
                        let t = find("topic");
                        if t.is_empty() {
                            None
                        } else {
                            Some(t)
                        }
                    },
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: find("unread_count").parse().unwrap_or(0),
                    member_count: find("member_count").parse().unwrap_or(0),
                    last_message: {
                        let m = find("last_message");
                        if m.is_empty() {
                            None
                        } else {
                            Some(m)
                        }
                    },
                    last_message_time: find("last_message_time").parse().ok(),
                    last_activity: find("last_activity").parse().unwrap_or(0),
                }
            })
            .collect()
    }
}

/// Query for messages in a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MessagesQuery {
    /// Channel ID
    pub channel_id: String,
    /// Maximum number of messages to return
    pub limit: u32,
    /// Offset for pagination (message ID to start after)
    pub after: Option<String>,
    /// Only return unread messages
    pub unread_only: bool,
}

impl Query for MessagesQuery {
    type Result = Vec<crate::views::chat::Message>;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        vec![DatalogRule {
            head: "message($id, $channel, $sender, $content, $time)".to_string(),
            body: vec![
                "message($id, $channel, $sender, $content, $time)".to_string(),
                format!("$channel = \"{}\"", self.channel_id),
            ],
        }]
    }

    fn parse_results(_bindings: Vec<Vec<Binding>>) -> Self::Result {
        Vec::new()
    }
}

/// Query for guardians
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GuardiansQuery {
    /// Filter by status
    pub status: Option<String>,
    /// Include revoked guardians
    pub include_revoked: bool,
}

impl Query for GuardiansQuery {
    type Result = Vec<crate::views::recovery::Guardian>;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        vec![DatalogRule {
            head: "guardian($id, $name, $status)".to_string(),
            body: vec!["guardian($id, $name, $status)".to_string()],
        }]
    }

    fn parse_results(_bindings: Vec<Vec<Binding>>) -> Self::Result {
        Vec::new()
    }
}

/// Query for invitations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct InvitationsQuery {
    /// Filter by direction (sent/received)
    pub direction: Option<String>,
    /// Filter by status
    pub status: Option<String>,
    /// Filter by type
    pub invitation_type: Option<String>,
}

impl Query for InvitationsQuery {
    type Result = Vec<crate::views::invitations::Invitation>;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        vec![DatalogRule {
            head: "invitation($id, $type, $status, $from, $to)".to_string(),
            body: vec!["invitation($id, $type, $status, $from, $to)".to_string()],
        }]
    }

    fn parse_results(_bindings: Vec<Vec<Binding>>) -> Self::Result {
        Vec::new()
    }
}

/// Query for contacts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ContactsQuery {
    /// Search filter
    pub search: Option<String>,
    /// Filter guardians only
    pub guardians_only: bool,
    /// Filter residents only
    pub residents_only: bool,
}

impl Query for ContactsQuery {
    type Result = Vec<crate::views::contacts::Contact>;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        vec![DatalogRule {
            head: "contact($id, $petname, $is_guardian, $is_resident)".to_string(),
            body: vec!["contact($id, $petname, $is_guardian, $is_resident)".to_string()],
        }]
    }

    fn parse_results(_bindings: Vec<Vec<Binding>>) -> Self::Result {
        Vec::new()
    }
}

/// Query for unread message count
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UnreadCountQuery {
    /// Optional channel filter
    pub channel_id: Option<String>,
}

impl Query for UnreadCountQuery {
    type Result = u32;

    fn to_datalog(&self) -> Vec<DatalogRule> {
        vec![DatalogRule {
            head: "unread_count($count)".to_string(),
            body: vec!["count(message($id, _, _, _, _), $count)".to_string()],
        }]
    }

    fn parse_results(_bindings: Vec<Vec<Binding>>) -> Self::Result {
        0
    }
}

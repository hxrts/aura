//! Query type definitions
//!
//! Each query type implements the `aura_core::Query` trait for typed Datalog compilation.
//!
//! ## Query Structure
//!
//! Each query provides:
//! - `to_datalog()` - Compiles to a DatalogProgram with rules and initial facts
//! - `required_capabilities()` - Biscuit capabilities for authorization
//! - `dependencies()` - Fact predicates for automatic invalidation
//! - `parse()` - Parses DatalogBindings to typed results

use serde::{Deserialize, Serialize};

use aura_core::{
    crypto::hash::hash,
    identifiers::{AuthorityId, ChannelId},
    query::{
        DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate,
        Query, QueryCapability, QueryParseError,
    },
};

// =============================================================================
// Helper Functions
// =============================================================================

/// Helper to extract a string value from a DatalogRow
fn get_string(row: &aura_core::query::DatalogRow, key: &str) -> String {
    row.get(key)
        .and_then(|v| match v {
            DatalogValue::String(s) => Some(s.clone()),
            DatalogValue::Symbol(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Helper to extract an optional string value from a DatalogRow
fn get_optional_string(row: &aura_core::query::DatalogRow, key: &str) -> Option<String> {
    let s = get_string(row, key);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Helper to extract an AuthorityId from a DatalogRow, parsing or hashing the string
fn get_authority_id(row: &aura_core::query::DatalogRow, key: &str) -> AuthorityId {
    let s = get_string(row, key);
    s.parse::<AuthorityId>().unwrap_or_default()
}

/// Helper to extract an optional AuthorityId from a DatalogRow
fn get_optional_authority_id(row: &aura_core::query::DatalogRow, key: &str) -> Option<AuthorityId> {
    let s = get_string(row, key);
    if s.is_empty() {
        None
    } else {
        Some(s.parse::<AuthorityId>().unwrap_or_default())
    }
}

/// Helper to extract a ChannelId from a DatalogRow, parsing or hashing the string
fn get_channel_id(row: &aura_core::query::DatalogRow, key: &str) -> ChannelId {
    let s = get_string(row, key);
    s.parse::<ChannelId>()
        .unwrap_or_else(|_| ChannelId::from_bytes(hash(s.as_bytes())))
}

/// Helper to extract an optional ChannelId from a DatalogRow
fn get_optional_channel_id(row: &aura_core::query::DatalogRow, key: &str) -> Option<ChannelId> {
    let s = get_string(row, key);
    if s.is_empty() {
        None
    } else {
        Some(
            s.parse::<ChannelId>()
                .unwrap_or_else(|_| ChannelId::from_bytes(hash(s.as_bytes()))),
        )
    }
}

/// Helper to extract an integer value from a DatalogRow
fn get_int(row: &aura_core::query::DatalogRow, key: &str) -> i64 {
    row.get(key)
        .and_then(|v| match v {
            DatalogValue::Integer(i) => Some(*i),
            DatalogValue::String(s) => s.parse().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

/// Helper to extract a boolean value from a DatalogRow
fn get_bool(row: &aura_core::query::DatalogRow, key: &str) -> bool {
    row.get(key)
        .map(|v| match v {
            DatalogValue::Boolean(b) => *b,
            DatalogValue::String(s) => s == "true",
            _ => false,
        })
        .unwrap_or(false)
}

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

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "channel",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("type"),
                DatalogValue::var("topic"),
                DatalogValue::var("archived"),
            ],
        )];

        // Add channel_type filter if specified
        if let Some(ref channel_type) = self.channel_type {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("type"),
                    DatalogValue::String(channel_type.clone()),
                ],
            ));
        }

        // Filter out archived unless include_archived is true
        if !self.include_archived {
            body.push(DatalogFact::new(
                "eq",
                vec![DatalogValue::var("archived"), DatalogValue::Boolean(false)],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("topic"),
                    DatalogValue::var("archived"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("channels")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("channel")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::{Channel, ChannelType};

        let channels = bindings
            .rows
            .into_iter()
            .map(|row| {
                let channel_type = match get_string(&row, "type").as_str() {
                    "DirectMessage" => ChannelType::DirectMessage,
                    "Guardian" => ChannelType::Guardian,
                    _ => ChannelType::Block,
                };

                Channel {
                    id: get_channel_id(&row, "id"),
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_count: get_int(&row, "member_count") as u32,
                    last_message: get_optional_string(&row, "last_message"),
                    last_message_time: {
                        let ts = get_int(&row, "last_message_time");
                        if ts > 0 {
                            Some(ts as u64)
                        } else {
                            None
                        }
                    },
                    last_activity: get_int(&row, "last_activity") as u64,
                }
            })
            .collect();

        Ok(channels)
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

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![
            DatalogFact::new(
                "message",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("channel"),
                    DatalogValue::var("sender"),
                    DatalogValue::var("sender_name"),
                    DatalogValue::var("content"),
                    DatalogValue::var("timestamp"),
                ],
            ),
            DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("channel"),
                    DatalogValue::String(self.channel_id.clone()),
                ],
            ),
        ];

        if self.unread_only {
            body.push(DatalogFact::new(
                "message_unread",
                vec![DatalogValue::var("id")],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("sender"),
                    DatalogValue::var("sender_name"),
                    DatalogValue::var("content"),
                    DatalogValue::var("timestamp"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![
            QueryCapability::read("messages"),
            QueryCapability::read(format!("channel:{}", self.channel_id)),
        ]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::with_args("message", vec![("channel", &self.channel_id)]),
            FactPredicate::new("message_unread"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::Message;

        let messages = bindings
            .rows
            .into_iter()
            .map(|row| Message {
                id: get_string(&row, "id"),
                channel_id: get_channel_id(&row, "channel"),
                sender_id: get_authority_id(&row, "sender"),
                sender_name: get_string(&row, "sender_name"),
                content: get_string(&row, "content"),
                timestamp: get_int(&row, "timestamp") as u64,
                reply_to: get_optional_string(&row, "reply_to"),
                is_own: false, // Determined at render time
                is_read: get_bool(&row, "is_read"),
            })
            .collect();

        Ok(messages)
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

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "guardian",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("status"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(ref status) = self.status {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String(status.clone()),
                ],
            ));
        }

        if !self.include_revoked {
            body.push(DatalogFact::new(
                "neq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String("revoked".to_string()),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("status"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("guardians")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("guardian")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::recovery::{Guardian, GuardianStatus};

        let guardians = bindings
            .rows
            .into_iter()
            .map(|row| {
                let status = match get_string(&row, "status").as_str() {
                    "active" => GuardianStatus::Active,
                    "pending" => GuardianStatus::Pending,
                    "revoked" => GuardianStatus::Revoked,
                    "offline" => GuardianStatus::Offline,
                    _ => GuardianStatus::Pending,
                };

                let last_seen = {
                    let ts = get_int(&row, "last_seen");
                    if ts > 0 {
                        Some(ts as u64)
                    } else {
                        None
                    }
                };

                Guardian {
                    id: get_authority_id(&row, "id"),
                    name: get_string(&row, "name"),
                    status,
                    added_at: get_int(&row, "added_at") as u64,
                    last_seen,
                }
            })
            .collect();

        Ok(guardians)
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
    type Result = crate::views::invitations::InvitationsState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "invitation",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("type"),
                DatalogValue::var("status"),
                DatalogValue::var("direction"),
                DatalogValue::var("from"),
                DatalogValue::var("to"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(ref direction) = self.direction {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("direction"),
                    DatalogValue::String(direction.clone()),
                ],
            ));
        }

        if let Some(ref status) = self.status {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String(status.clone()),
                ],
            ));
        }

        if let Some(ref inv_type) = self.invitation_type {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("type"),
                    DatalogValue::String(inv_type.clone()),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("type"),
                    DatalogValue::var("status"),
                    DatalogValue::var("direction"),
                    DatalogValue::var("from"),
                    DatalogValue::var("to"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("invitations")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("invitation")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::invitations::{
            Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
        };

        let invitations: Vec<Invitation> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let inv_type = match get_string(&row, "type").as_str() {
                    "guardian" => InvitationType::Guardian,
                    "chat" => InvitationType::Chat,
                    "block" => InvitationType::Block,
                    _ => InvitationType::Block,
                };

                let status = match get_string(&row, "status").as_str() {
                    "pending" => InvitationStatus::Pending,
                    "accepted" => InvitationStatus::Accepted,
                    "rejected" => InvitationStatus::Rejected,
                    "expired" => InvitationStatus::Expired,
                    "revoked" => InvitationStatus::Revoked,
                    _ => InvitationStatus::Pending,
                };

                let direction = match get_string(&row, "direction").as_str() {
                    "sent" => InvitationDirection::Sent,
                    "received" => InvitationDirection::Received,
                    _ => InvitationDirection::Received,
                };

                let expires_at = {
                    let ts = get_int(&row, "expires_at");
                    if ts > 0 {
                        Some(ts as u64)
                    } else {
                        None
                    }
                };

                Invitation {
                    id: get_string(&row, "id"),
                    invitation_type: inv_type,
                    status,
                    direction,
                    from_id: get_authority_id(&row, "from_id"),
                    from_name: get_string(&row, "from"),
                    to_id: get_optional_authority_id(&row, "to_id"),
                    to_name: get_optional_string(&row, "to"),
                    created_at: get_int(&row, "created_at") as u64,
                    expires_at,
                    message: get_optional_string(&row, "message"),
                    block_id: get_optional_channel_id(&row, "block_id"),
                    block_name: get_optional_string(&row, "block_name"),
                }
            })
            .collect();

        // Partition invitations by status and direction
        let pending: Vec<Invitation> = invitations
            .iter()
            .filter(|i| {
                i.status == InvitationStatus::Pending
                    && i.direction == InvitationDirection::Received
            })
            .cloned()
            .collect();
        let sent: Vec<Invitation> = invitations
            .iter()
            .filter(|i| {
                i.status == InvitationStatus::Pending && i.direction == InvitationDirection::Sent
            })
            .cloned()
            .collect();
        let history: Vec<Invitation> = invitations
            .iter()
            .filter(|i| i.status != InvitationStatus::Pending)
            .cloned()
            .collect();

        let pending_count = pending.len() as u32;

        Ok(InvitationsState {
            pending,
            sent,
            history,
            pending_count,
        })
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
    type Result = crate::views::contacts::ContactsState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "contact",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("nickname"),
                DatalogValue::var("suggested_name"),
                DatalogValue::var("is_guardian"),
                DatalogValue::var("is_resident"),
                DatalogValue::var("last_interaction"),
                DatalogValue::var("is_online"),
            ],
        )];

        if self.guardians_only {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("is_guardian"),
                    DatalogValue::Boolean(true),
                ],
            ));
        }

        if self.residents_only {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("is_resident"),
                    DatalogValue::Boolean(true),
                ],
            ));
        }

        // Note: search filtering is typically done client-side or via a separate text search index

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("nickname"),
                    DatalogValue::var("suggested_name"),
                    DatalogValue::var("is_guardian"),
                    DatalogValue::var("is_resident"),
                    DatalogValue::var("last_interaction"),
                    DatalogValue::var("is_online"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("contacts")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("contact")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::contacts::{Contact, ContactsState};

        let contacts: Vec<Contact> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let last_interaction = {
                    let ts = get_int(&row, "last_interaction");
                    if ts > 0 {
                        Some(ts as u64)
                    } else {
                        None
                    }
                };

                Contact {
                    id: get_authority_id(&row, "id"),
                    nickname: get_string(&row, "nickname"),
                    suggested_name: get_optional_string(&row, "suggested_name"),
                    is_guardian: get_bool(&row, "is_guardian"),
                    is_resident: get_bool(&row, "is_resident"),
                    last_interaction,
                    is_online: get_bool(&row, "is_online"),
                }
            })
            .collect();

        Ok(ContactsState {
            contacts,
            selected_contact_id: None, // Set by caller
            search_filter: None,       // Set by caller
        })
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

    fn to_datalog(&self) -> DatalogProgram {
        let body = if let Some(ref channel_id) = self.channel_id {
            vec![
                DatalogFact::new("message_unread", vec![DatalogValue::var("id")]),
                DatalogFact::new(
                    "message",
                    vec![
                        DatalogValue::var("id"),
                        DatalogValue::String(channel_id.clone()),
                        DatalogValue::var("_sender"),
                        DatalogValue::var("_content"),
                        DatalogValue::var("_time"),
                    ],
                ),
            ]
        } else {
            vec![DatalogFact::new(
                "message_unread",
                vec![DatalogValue::var("id")],
            )]
        };

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new("result", vec![DatalogValue::var("id")]),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("messages")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("message_unread")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        // Count is the number of result rows
        Ok(bindings.rows.len() as u32)
    }
}

/// Query for recovery status
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryQuery;

impl Query for RecoveryQuery {
    type Result = crate::views::recovery::RecoveryState;

    fn to_datalog(&self) -> DatalogProgram {
        // Query recovery_config for threshold and guardian_count,
        // plus join with guardians for the full list
        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("threshold"),
                    DatalogValue::var("guardian_count"),
                ],
            ),
            body: vec![DatalogFact::new(
                "recovery_config",
                vec![
                    DatalogValue::var("threshold"),
                    DatalogValue::var("guardian_count"),
                ],
            )],
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("recovery")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("recovery_config"),
            FactPredicate::new("guardian"),
            FactPredicate::new("recovery_process"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::recovery::RecoveryState;

        // RecoveryQuery returns configuration only; guardians and active recovery
        // state are populated via separate queries (GuardiansQuery, etc.)
        if let Some(row) = bindings.rows.first() {
            Ok(RecoveryState {
                guardians: Vec::new(),
                threshold: get_int(row, "threshold") as u32,
                guardian_count: get_int(row, "guardian_count") as u32,
                active_recovery: None,
                pending_requests: Vec::new(),
            })
        } else {
            Ok(RecoveryState::default())
        }
    }
}

/// Query for blocks state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BlocksQuery {
    /// Filter by block ID (optional)
    pub block_id: Option<String>,
    /// Include only blocks where user is admin
    pub admin_only: bool,
}

impl Query for BlocksQuery {
    type Result = crate::views::block::BlocksState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "block",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("is_primary"),
                DatalogValue::var("my_role"),
                DatalogValue::var("resident_count"),
                DatalogValue::var("online_count"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(ref block_id) = self.block_id {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::String(block_id.clone()),
                ],
            ));
        }

        if self.admin_only {
            // Match admin or owner roles
            body.push(DatalogFact::new(
                "in",
                vec![
                    DatalogValue::var("my_role"),
                    DatalogValue::Symbol("admin".to_string()),
                    DatalogValue::Symbol("owner".to_string()),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("is_primary"),
                    DatalogValue::var("my_role"),
                    DatalogValue::var("resident_count"),
                    DatalogValue::var("online_count"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("blocks")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("block"),
            FactPredicate::new("block_resident"),
            FactPredicate::new("block_role"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::block::{BlockState, BlocksState, ResidentRole};
        use crate::workflows::budget::BlockFlowBudget;
        use std::collections::HashMap;

        let blocks_list: Vec<BlockState> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let my_role = match get_string(&row, "my_role").as_str() {
                    "owner" => ResidentRole::Owner,
                    "admin" => ResidentRole::Admin,
                    _ => ResidentRole::Resident,
                };

                BlockState {
                    id: get_channel_id(&row, "id"),
                    name: get_string(&row, "name"),
                    residents: Vec::new(), // Populated by separate query
                    my_role,
                    storage: BlockFlowBudget::default(),
                    online_count: get_int(&row, "online_count") as u32,
                    resident_count: get_int(&row, "resident_count") as u32,
                    is_primary: get_bool(&row, "is_primary"),
                    topic: get_optional_string(&row, "topic"),
                    pinned_messages: Vec::new(),
                    pinned_metadata: Default::default(),
                    mode_flags: None,
                    ban_list: Default::default(),
                    mute_list: Default::default(),
                    kick_log: Vec::new(),
                    created_at: get_int(&row, "created_at") as u64,
                    context_id: get_string(&row, "context_id"),
                }
            })
            .collect();

        // Convert to HashMap and find current block
        let mut blocks: HashMap<ChannelId, BlockState> = HashMap::new();
        let mut current_block_id: Option<ChannelId> = None;

        for block in blocks_list {
            if block.is_primary && current_block_id.is_none() {
                current_block_id = Some(block.id);
            }
            blocks.insert(block.id, block);
        }

        // If no primary block, select first block
        if current_block_id.is_none() {
            current_block_id = blocks.keys().next().cloned();
        }

        Ok(BlocksState {
            blocks,
            current_block_id,
        })
    }
}

/// Query for neighborhood state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborhoodQuery {
    /// Current position block ID (None = home block)
    pub position_block_id: Option<String>,
    /// Maximum traversal depth
    pub max_depth: Option<u32>,
}

impl Query for NeighborhoodQuery {
    type Result = crate::views::neighborhood::NeighborhoodState;

    fn to_datalog(&self) -> DatalogProgram {
        // Query neighbor_block facts for the neighborhood view
        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("adjacency"),
                    DatalogValue::var("shared_contacts"),
                    DatalogValue::var("can_traverse"),
                ],
            ),
            body: vec![DatalogFact::new(
                "neighbor_block",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("adjacency"),
                    DatalogValue::var("shared_contacts"),
                    DatalogValue::var("can_traverse"),
                ],
            )],
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("neighborhood")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("neighbor_block"),
            FactPredicate::new("block_adjacency"),
            FactPredicate::new("shared_contact"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::neighborhood::{AdjacencyType, NeighborBlock, NeighborhoodState};

        let neighbors = bindings
            .rows
            .into_iter()
            .map(|row| {
                let adjacency = match get_string(&row, "adjacency").as_str() {
                    "direct" => AdjacencyType::Direct,
                    "two_hop" => AdjacencyType::TwoHop,
                    _ => AdjacencyType::Distant,
                };

                NeighborBlock {
                    id: get_channel_id(&row, "id"),
                    name: get_string(&row, "name"),
                    adjacency,
                    shared_contacts: get_int(&row, "shared_contacts") as u32,
                    resident_count: {
                        let count = get_int(&row, "resident_count");
                        if count > 0 {
                            Some(count as u32)
                        } else {
                            None
                        }
                    },
                    can_traverse: get_bool(&row, "can_traverse"),
                }
            })
            .collect();

        Ok(NeighborhoodState {
            home_block_id: ChannelId::default(), // Set by caller
            home_block_name: String::new(),      // Set by caller
            position: None,
            neighbors,
            max_depth: 3,
            loading: false,
        })
    }
}

/// Query for chat state (combines channels and messages)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatQuery {
    /// Currently selected channel ID
    pub selected_channel_id: Option<String>,
}

impl Query for ChatQuery {
    type Result = crate::views::chat::ChatState;

    fn to_datalog(&self) -> DatalogProgram {
        // Query channels for the chat state
        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("unread_count"),
                ],
            ),
            body: vec![DatalogFact::new(
                "channel",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("topic"),
                    DatalogValue::var("archived"),
                ],
            )],
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![
            QueryCapability::read("channels"),
            QueryCapability::read("messages"),
        ]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("channel"),
            FactPredicate::new("message"),
            FactPredicate::new("message_unread"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::{Channel, ChannelType, ChatState};

        let channels: Vec<Channel> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let channel_type = match get_string(&row, "type").as_str() {
                    "DirectMessage" => ChannelType::DirectMessage,
                    "Guardian" => ChannelType::Guardian,
                    _ => ChannelType::Block,
                };

                Channel {
                    id: get_channel_id(&row, "id"),
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_count: 0,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                }
            })
            .collect();

        Ok(ChatState {
            channels,
            selected_channel_id: None, // Set by caller
            messages: Vec::new(),      // Loaded separately
            total_unread: 0,           // Calculated from channels
            loading_more: false,
            has_more: false,
        })
    }
}

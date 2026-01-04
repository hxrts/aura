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
    identifiers::{AuthorityId, ChannelId, ContextId},
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

/// Helper to extract an AuthorityId from a DatalogRow.
fn get_authority_id(
    row: &aura_core::query::DatalogRow,
    key: &str,
) -> Result<AuthorityId, QueryParseError> {
    let s = get_string(row, key);
    if s.is_empty() {
        return Err(QueryParseError::MissingField {
            field: key.to_string(),
        });
    }
    s.parse::<AuthorityId>()
        .map_err(|e| QueryParseError::InvalidValue {
            field: key.to_string(),
            reason: format!("{e}"),
        })
}

/// Helper to extract an optional AuthorityId from a DatalogRow.
fn get_optional_authority_id(
    row: &aura_core::query::DatalogRow,
    key: &str,
) -> Result<Option<AuthorityId>, QueryParseError> {
    let s = get_string(row, key);
    if s.is_empty() {
        Ok(None)
    } else {
        s.parse::<AuthorityId>()
            .map(Some)
            .map_err(|e| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: format!("{e}"),
            })
    }
}

/// Helper to extract a ChannelId from a DatalogRow.
fn get_channel_id(
    row: &aura_core::query::DatalogRow,
    key: &str,
) -> Result<ChannelId, QueryParseError> {
    let s = get_string(row, key);
    if s.is_empty() {
        return Err(QueryParseError::MissingField {
            field: key.to_string(),
        });
    }
    s.parse::<ChannelId>()
        .map_err(|e| QueryParseError::InvalidValue {
            field: key.to_string(),
            reason: format!("{e}"),
        })
}

/// Helper to extract an optional ChannelId from a DatalogRow.
fn get_optional_channel_id(
    row: &aura_core::query::DatalogRow,
    key: &str,
) -> Result<Option<ChannelId>, QueryParseError> {
    let s = get_string(row, key);
    if s.is_empty() {
        Ok(None)
    } else {
        s.parse::<ChannelId>()
            .map(Some)
            .map_err(|e| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: format!("{e}"),
            })
    }
}

/// Helper to extract an optional ContextId from a DatalogRow.
fn get_optional_context_id(
    row: &aura_core::query::DatalogRow,
    key: &str,
) -> Result<Option<ContextId>, QueryParseError> {
    let s = get_string(row, key);
    if s.is_empty() {
        Ok(None)
    } else {
        s.parse::<ContextId>()
            .map(Some)
            .map_err(|e| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: format!("{e}"),
            })
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
    pub channel_type: Option<crate::views::chat::ChannelType>,
    /// Include archived channels
    pub include_archived: bool,
}

impl Query for ChannelsQuery {
    type Result = Vec<crate::views::chat::Channel>;

    fn to_datalog(&self) -> DatalogProgram {
        use crate::views::chat::ChannelType;

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
        if let Some(channel_type) = self.channel_type {
            let channel_type = match channel_type {
                ChannelType::DirectMessage => Some("DirectMessage"),
                ChannelType::Guardian => Some("Guardian"),
                ChannelType::Home => Some("Home"),
                ChannelType::All => None,
            };
            if let Some(channel_type) = channel_type {
                body.push(DatalogFact::new(
                    "eq",
                    vec![
                        DatalogValue::var("type"),
                        DatalogValue::String(channel_type.to_string()),
                    ],
                ));
            }
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
                    _ => ChannelType::Home,
                };

                Ok(Channel {
                    id: get_channel_id(&row, "id")?,
                    context_id: None,
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_ids: Vec::new(),
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
                    last_finalized_epoch: 0,
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

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
            .map(|row| {
                Ok(Message {
                    id: get_string(&row, "id"),
                    channel_id: get_channel_id(&row, "channel")?,
                    sender_id: get_authority_id(&row, "sender")?,
                    sender_name: get_string(&row, "sender_name"),
                    content: get_string(&row, "content"),
                    timestamp: get_int(&row, "timestamp") as u64,
                    reply_to: get_optional_string(&row, "reply_to"),
                    is_own: false, // Determined at render time
                    is_read: get_bool(&row, "is_read"),
                    delivery_status: Default::default(),
                    epoch_hint: None, // Not available from Datalog query
                    is_finalized: false,
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

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

                Ok(Guardian {
                    id: get_authority_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    status,
                    added_at: get_int(&row, "added_at") as u64,
                    last_seen,
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

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
                    "home" => InvitationType::Home,
                    _ => InvitationType::Home,
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

                Ok(Invitation {
                    id: get_string(&row, "id"),
                    invitation_type: inv_type,
                    status,
                    direction,
                    from_id: get_authority_id(&row, "from_id")?,
                    from_name: get_string(&row, "from"),
                    to_id: get_optional_authority_id(&row, "to_id")?,
                    to_name: get_optional_string(&row, "to"),
                    created_at: get_int(&row, "created_at") as u64,
                    expires_at,
                    message: get_optional_string(&row, "message"),
                    home_id: get_optional_channel_id(&row, "home_id")?,
                    home_name: get_optional_string(&row, "home_name"),
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

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

        Ok(InvitationsState::from_parts(pending, sent, history))
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
        use crate::views::contacts::{Contact, ContactsState, ReadReceiptPolicy};

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

                Ok(Contact {
                    id: get_authority_id(&row, "id")?,
                    nickname: get_string(&row, "nickname"),
                    suggested_name: get_optional_string(&row, "suggested_name"),
                    is_guardian: get_bool(&row, "is_guardian"),
                    is_resident: get_bool(&row, "is_resident"),
                    last_interaction,
                    is_online: get_bool(&row, "is_online"),
                    read_receipt_policy: ReadReceiptPolicy::default(),
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

        Ok(ContactsState::from_iter(contacts))
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
        // plus join with guardian contacts (is_guardian = true).
        DatalogProgram::new(vec![
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                        DatalogValue::var("contact_id"),
                        DatalogValue::var("contact_nickname"),
                        DatalogValue::var("contact_suggested"),
                        DatalogValue::var("contact_last_interaction"),
                    ],
                ),
                body: vec![
                    DatalogFact::new(
                        "recovery_config",
                        vec![
                            DatalogValue::var("threshold"),
                            DatalogValue::var("guardian_count"),
                        ],
                    ),
                    DatalogFact::new(
                        "contact",
                        vec![
                            DatalogValue::var("contact_id"),
                            DatalogValue::var("contact_nickname"),
                            DatalogValue::var("contact_suggested"),
                            DatalogValue::var("is_guardian"),
                            DatalogValue::var("is_resident"),
                            DatalogValue::var("contact_last_interaction"),
                            DatalogValue::var("is_online"),
                        ],
                    ),
                    DatalogFact::new(
                        "eq",
                        vec![
                            DatalogValue::var("is_guardian"),
                            DatalogValue::Boolean(true),
                        ],
                    ),
                ],
            },
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::Integer(0),
                        DatalogValue::Integer(0),
                        DatalogValue::var("contact_id"),
                        DatalogValue::var("contact_nickname"),
                        DatalogValue::var("contact_suggested"),
                        DatalogValue::var("contact_last_interaction"),
                    ],
                ),
                body: vec![
                    DatalogFact::new(
                        "contact",
                        vec![
                            DatalogValue::var("contact_id"),
                            DatalogValue::var("contact_nickname"),
                            DatalogValue::var("contact_suggested"),
                            DatalogValue::var("is_guardian"),
                            DatalogValue::var("is_resident"),
                            DatalogValue::var("contact_last_interaction"),
                            DatalogValue::var("is_online"),
                        ],
                    ),
                    DatalogFact::new(
                        "eq",
                        vec![
                            DatalogValue::var("is_guardian"),
                            DatalogValue::Boolean(true),
                        ],
                    ),
                ],
            },
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                        DatalogValue::String(String::new()),
                        DatalogValue::String(String::new()),
                        DatalogValue::String(String::new()),
                        DatalogValue::Integer(0),
                    ],
                ),
                body: vec![DatalogFact::new(
                    "recovery_config",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                    ],
                )],
            },
        ])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("recovery")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("recovery_config"),
            FactPredicate::new("contact"),
            FactPredicate::new("recovery_process"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::recovery::RecoveryState;
        use crate::views::recovery::{Guardian, GuardianStatus};
        use std::collections::HashMap;

        if bindings.rows.is_empty() {
            return Ok(RecoveryState::default());
        }

        let mut guardians_by_id: HashMap<AuthorityId, Guardian> = HashMap::new();
        let mut threshold = 0u32;

        for row in bindings.rows.iter() {
            if threshold == 0 {
                threshold = get_int(row, "threshold") as u32;
            }
            // Note: guardian_count is now computed from guardians.len()

            let guardian_id = get_authority_id(row, "contact_id")?;
            let nickname = get_string(row, "contact_nickname");
            let suggested = get_string(row, "contact_suggested");
            let guardian_name = if !nickname.is_empty() {
                nickname
            } else if !suggested.is_empty() {
                suggested
            } else {
                guardian_id.to_string()
            };

            let status = GuardianStatus::Active;

            guardians_by_id.entry(guardian_id).or_insert(Guardian {
                id: guardian_id,
                name: guardian_name,
                status,
                added_at: get_int(row, "contact_last_interaction") as u64,
                last_seen: {
                    let ts = get_int(row, "contact_last_interaction");
                    if ts > 0 {
                        Some(ts as u64)
                    } else {
                        None
                    }
                },
            });
        }

        // Use the HashMap directly - RecoveryState now uses HashMap<AuthorityId, Guardian>
        Ok(RecoveryState::from_parts(
            guardians_by_id,
            threshold,
            None,
            Vec::new(),
            Vec::new(),
        ))
    }
}

/// Query for homes state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomesQuery {
    /// Filter by home ID (optional)
    pub home_id: Option<String>,
    /// Include only homes where user is admin
    pub admin_only: bool,
}

impl Query for HomesQuery {
    type Result = crate::views::home::HomesState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "home",
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

        if let Some(ref home_id) = self.home_id {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::String(home_id.clone()),
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
        vec![QueryCapability::read("homes")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("home"),
            FactPredicate::new("home_resident"),
            FactPredicate::new("home_role"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::home::{HomeState, HomesState, ResidentRole};
        use crate::workflows::budget::HomeFlowBudget;
        use std::collections::HashMap;

        let homes_list: Vec<HomeState> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let my_role = match get_string(&row, "my_role").as_str() {
                    "owner" => ResidentRole::Owner,
                    "admin" => ResidentRole::Admin,
                    _ => ResidentRole::Resident,
                };

                Ok(HomeState {
                    id: get_channel_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    residents: Vec::new(), // Populated by separate query
                    my_role,
                    storage: HomeFlowBudget::default(),
                    online_count: get_int(&row, "online_count") as u32,
                    resident_count: get_int(&row, "resident_count") as u32,
                    is_primary: get_bool(&row, "is_primary"),
                    topic: get_optional_string(&row, "topic"),
                    pinned_messages: Vec::new(),
                    pinned_metadata: HashMap::default(),
                    mode_flags: None,
                    ban_list: HashMap::default(),
                    mute_list: HashMap::default(),
                    kick_log: Vec::new(),
                    created_at: get_int(&row, "created_at") as u64,
                    context_id: get_optional_context_id(&row, "context_id")?,
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

        // Convert to HashMap and find current home
        let mut homes: HashMap<ChannelId, HomeState> = HashMap::new();
        let mut current_home_id: Option<ChannelId> = None;

        for home in homes_list {
            if home.is_primary && current_home_id.is_none() {
                current_home_id = Some(home.id);
            }
            homes.insert(home.id, home);
        }

        // If no primary home, select first home
        if current_home_id.is_none() {
            current_home_id = homes.keys().next().cloned();
        }

        Ok(HomesState::from_parts(homes, current_home_id))
    }
}

/// Query for neighborhood state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborhoodQuery {
    /// Current position home ID (None = local home)
    pub position_home_id: Option<String>,
    /// Maximum traversal depth
    pub max_depth: Option<u32>,
}

impl Query for NeighborhoodQuery {
    type Result = crate::views::neighborhood::NeighborhoodState;

    fn to_datalog(&self) -> DatalogProgram {
        // Query neighbor_home facts for the neighborhood view
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
                "neighbor_home",
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
            FactPredicate::new("neighbor_home"),
            FactPredicate::new("home_adjacency"),
            FactPredicate::new("shared_contact"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::neighborhood::{AdjacencyType, NeighborHome, NeighborhoodState};

        let neighbors = bindings
            .rows
            .into_iter()
            .map(|row| {
                let adjacency = match get_string(&row, "adjacency").as_str() {
                    "direct" => AdjacencyType::Direct,
                    "two_hop" => AdjacencyType::TwoHop,
                    _ => AdjacencyType::Distant,
                };

                Ok(NeighborHome {
                    id: get_channel_id(&row, "id")?,
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
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

        Ok(NeighborhoodState {
            home_home_id: ChannelId::default(), // Set by caller
            home_name: String::new(),           // Set by caller
            position: None,
            neighbors,
            max_depth: 3,
            loading: false,
            connected_peers: std::collections::HashSet::new(),
        })
    }
}

/// Query for chat state (combines channels and messages)
///
/// Note: Channel selection is UI-only state managed by the frontend.
/// Messages are loaded per-channel via `ChatState::messages_for_channel()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatQuery {}

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
                    _ => ChannelType::Home,
                };

                Ok(Channel {
                    id: get_channel_id(&row, "id")?,
                    context_id: None,
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_ids: Vec::new(),
                    member_count: 0,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                })
            })
            .collect::<Result<Vec<_>, QueryParseError>>()?;

        // Note: selected_channel_id is now UI state in the frontend
        // Messages are loaded per-channel via messages_for_channel()
        Ok(ChatState::from_channels(channels))
    }
}

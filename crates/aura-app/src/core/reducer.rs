//! # View Reducer
//!
//! Reduces journal facts into view state updates.
//!
//! This module implements the CQRS reduce step:
//! ```text
//! Intent → Authorize → Journal → [Reduce] → View → Sync
//! ```
//!
//! Facts are parsed from their content string format and applied
//! to the appropriate view state.

use crate::views::{Channel, ChannelType, Message};
use aura_core::crypto::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::TimeStamp;
use aura_journal::JournalFact;
use std::collections::HashMap;
use thiserror::Error;

/// Parse a string channel ID, falling back to hashing the string if not valid hash-id format.
///
/// Note: the app-level fact format stores `ContextId`s as strings, but the chat view uses
/// `ChannelId` (hash-id). This function provides a deterministic mapping.
fn parse_channel_id(s: &str) -> ChannelId {
    s.parse::<ChannelId>()
        .unwrap_or_else(|_| ChannelId::from_bytes(hash(s.as_bytes())))
}

fn parse_context_id(
    fact_type: &'static str,
    content: &str,
    raw: &str,
) -> Result<ContextId, ReduceError> {
    raw.parse::<ContextId>()
        .map_err(|_| ReduceError::InvalidId {
            fact_type,
            id_kind: "ContextId",
            value: raw.to_string(),
            content: content.to_string(),
        })
}

fn parse_authority_id(
    fact_type: &'static str,
    content: &str,
    raw: &str,
) -> Result<AuthorityId, ReduceError> {
    raw.parse::<AuthorityId>()
        .map_err(|_| ReduceError::InvalidId {
            fact_type,
            id_kind: "AuthorityId",
            value: raw.to_string(),
            content: content.to_string(),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactType {
    // View-relevant
    SendMessage,
    CreateChannel,
    JoinChannel,
    LeaveChannel,
    CloseChannel,
    UpdateChannel,
    SetTopic,
    SetBlockTopic,
    SetNickname,
    SetBlockName,
    GrantSteward,
    RevokeSteward,
    InitiateRecovery,
    ApproveRecovery,
    ToggleGuardian,
    SetGuardianThreshold,
    CreateInvitation,
    AcceptInvitation,
    RejectInvitation,

    // Known but currently not reduced into ViewState.
    MarkAsRead,
    EditMessage,
    DeleteMessage,
    InviteMember,
    RemoveMember,
    CreateAccount,
    CreateAuthority,
    AddDevice,
    RemoveDevice,
    UpdateThreshold,
    CompleteRecovery,
    RejectRecovery,
    RemoveContact,
    RevokeInvitation,
    CreateBlock,
    InviteToBlock,
    UpdateBlockStorage,
    BanUser,
    UnbanUser,
    MuteUser,
    UnmuteUser,
    KickUser,
    PinMessage,
    UnpinMessage,
    NavigateTo,
    GoBack,
    ReplaceAdmin,
    ProposeSnapshot,
    ShowAuthority,
    ListAuthorities,
    InspectContext,
    ShowReceipts,
    InspectAmpChannel,
    BumpChannelEpoch,
    CheckpointChannel,
    ProposeUpgrade,
    SetOtaPolicy,
    GetOtaStatus,
    OptInUpgrade,
    ListUpgradeProposals,
    GetUpgradeStats,
    StartNode,
    RunThreshold,
    InitAccount,
    GetStatus,
    GetVersion,
}

impl FactType {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "SendMessage" => Self::SendMessage,
            "CreateChannel" => Self::CreateChannel,
            "JoinChannel" => Self::JoinChannel,
            "LeaveChannel" => Self::LeaveChannel,
            "CloseChannel" => Self::CloseChannel,
            "UpdateChannel" => Self::UpdateChannel,
            "SetTopic" => Self::SetTopic,
            "SetBlockTopic" => Self::SetBlockTopic,
            "SetNickname" => Self::SetNickname,
            "SetBlockName" => Self::SetBlockName,
            "GrantSteward" => Self::GrantSteward,
            "RevokeSteward" => Self::RevokeSteward,
            "InitiateRecovery" => Self::InitiateRecovery,
            "ApproveRecovery" => Self::ApproveRecovery,
            "ToggleGuardian" => Self::ToggleGuardian,
            "SetGuardianThreshold" => Self::SetGuardianThreshold,
            "CreateInvitation" => Self::CreateInvitation,
            "AcceptInvitation" => Self::AcceptInvitation,
            "RejectInvitation" => Self::RejectInvitation,

            "MarkAsRead" => Self::MarkAsRead,
            "EditMessage" => Self::EditMessage,
            "DeleteMessage" => Self::DeleteMessage,
            "InviteMember" => Self::InviteMember,
            "RemoveMember" => Self::RemoveMember,
            "CreateAccount" => Self::CreateAccount,
            "CreateAuthority" => Self::CreateAuthority,
            "AddDevice" => Self::AddDevice,
            "RemoveDevice" => Self::RemoveDevice,
            "UpdateThreshold" => Self::UpdateThreshold,
            "CompleteRecovery" => Self::CompleteRecovery,
            "RejectRecovery" => Self::RejectRecovery,
            "RemoveContact" => Self::RemoveContact,
            "RevokeInvitation" => Self::RevokeInvitation,
            "CreateBlock" => Self::CreateBlock,
            "InviteToBlock" => Self::InviteToBlock,
            "UpdateBlockStorage" => Self::UpdateBlockStorage,
            "BanUser" => Self::BanUser,
            "UnbanUser" => Self::UnbanUser,
            "MuteUser" => Self::MuteUser,
            "UnmuteUser" => Self::UnmuteUser,
            "KickUser" => Self::KickUser,
            "PinMessage" => Self::PinMessage,
            "UnpinMessage" => Self::UnpinMessage,
            "NavigateTo" => Self::NavigateTo,
            "GoBack" => Self::GoBack,
            "ReplaceAdmin" => Self::ReplaceAdmin,
            "ProposeSnapshot" => Self::ProposeSnapshot,
            "ShowAuthority" => Self::ShowAuthority,
            "ListAuthorities" => Self::ListAuthorities,
            "InspectContext" => Self::InspectContext,
            "ShowReceipts" => Self::ShowReceipts,
            "InspectAmpChannel" => Self::InspectAmpChannel,
            "BumpChannelEpoch" => Self::BumpChannelEpoch,
            "CheckpointChannel" => Self::CheckpointChannel,
            "ProposeUpgrade" => Self::ProposeUpgrade,
            "SetOtaPolicy" => Self::SetOtaPolicy,
            "GetOtaStatus" => Self::GetOtaStatus,
            "OptInUpgrade" => Self::OptInUpgrade,
            "ListUpgradeProposals" => Self::ListUpgradeProposals,
            "GetUpgradeStats" => Self::GetUpgradeStats,
            "StartNode" => Self::StartNode,
            "RunThreshold" => Self::RunThreshold,
            "InitAccount" => Self::InitAccount,
            "GetStatus" => Self::GetStatus,
            "GetVersion" => Self::GetVersion,
            _ => return None,
        })
    }

    fn is_view_relevant(&self) -> bool {
        matches!(
            self,
            Self::SendMessage
                | Self::CreateChannel
                | Self::JoinChannel
                | Self::LeaveChannel
                | Self::CloseChannel
                | Self::UpdateChannel
                | Self::SetTopic
                | Self::SetBlockTopic
                | Self::SetNickname
                | Self::SetBlockName
                | Self::GrantSteward
                | Self::RevokeSteward
                | Self::InitiateRecovery
                | Self::ApproveRecovery
                | Self::ToggleGuardian
                | Self::SetGuardianThreshold
                | Self::CreateInvitation
                | Self::AcceptInvitation
                | Self::RejectInvitation
        )
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReduceError {
    #[error("Malformed fact content (missing '::'): {content}")]
    MissingSeparator { content: String },

    #[error("Unknown fact type '{fact_type}' in content: {content}")]
    UnknownFactType { fact_type: String, content: String },

    #[error("Missing required param '{key}' for '{fact_type}' (content: {content})")]
    MissingParam {
        fact_type: &'static str,
        key: &'static str,
        content: String,
    },

    #[error("Invalid {id_kind} '{value}' for '{fact_type}' (content: {content})")]
    InvalidId {
        fact_type: &'static str,
        id_kind: &'static str,
        value: String,
        content: String,
    },

    #[error("Invalid boolean '{value}' for '{key}' in '{fact_type}' (content: {content})")]
    InvalidBool {
        fact_type: &'static str,
        key: &'static str,
        value: String,
        content: String,
    },

    #[error("Invalid number '{value}' for '{key}' in '{fact_type}' (content: {content})")]
    InvalidNumber {
        fact_type: &'static str,
        key: &'static str,
        value: String,
        content: String,
    },

    #[error("Malformed params for '{fact_type}' (content: {content}): {reason}")]
    ParamParse {
        fact_type: &'static str,
        reason: String,
        content: String,
    },

    #[error("Invalid percent-encoding in '{fact_type}' (content: {content}): {reason}")]
    PercentDecode {
        fact_type: &'static str,
        reason: String,
        content: String,
    },
}

/// Result of reducing a fact.
#[derive(Debug, Clone)]
pub enum ViewDelta {
    /// A new message was sent.
    MessageSent {
        channel_id: ChannelId,
        message: Message,
    },
    /// A channel was created.
    ChannelCreated { channel: Channel },
    /// A channel was joined.
    ChannelJoined { channel_id: ChannelId },
    /// A channel was left.
    ChannelLeft { channel_id: ChannelId },
    /// A channel was closed/archived.
    ChannelClosed { channel_id: ChannelId },
    /// Channel topic was updated.
    TopicUpdated {
        channel_id: ChannelId,
        topic: String,
    },
    /// A nickname was set for a contact.
    NicknameSet {
        target: AuthorityId,
        nickname: String,
    },
    /// A block name was set.
    BlockNameSet { block_id: ChannelId, name: String },
    /// Steward role granted (promote to Admin).
    StewardGranted {
        context_id: ContextId,
        target_id: AuthorityId,
    },
    /// Steward role revoked (demote to Resident).
    StewardRevoked {
        context_id: ContextId,
        target_id: AuthorityId,
    },
    /// A recovery request was initiated.
    RecoveryRequested { session_id: String },
    /// A guardian approval was granted.
    GuardianApproved { guardian_id: AuthorityId },
    /// Guardian status was toggled for a contact.
    GuardianToggled {
        contact_id: AuthorityId,
        is_guardian: bool,
    },
    /// Guardian threshold was configured.
    GuardianThresholdSet { threshold: u32 },
    /// An invitation was created.
    InvitationCreated { invitation_id: String },
    /// An invitation was accepted.
    InvitationAccepted { invitation_id: String },
    /// An invitation was rejected.
    InvitationRejected { invitation_id: String },
}

/// Reduce a journal fact into a view delta.
///
/// Returns `Ok(None)` for known fact types that do not affect the app's `ViewState`.
pub fn reduce_fact(
    fact: &JournalFact,
    own_authority: &AuthorityId,
) -> Result<Option<ViewDelta>, ReduceError> {
    let content = &fact.content;

    let (fact_type_raw, params_raw) =
        content
            .split_once("::")
            .ok_or_else(|| ReduceError::MissingSeparator {
                content: content.clone(),
            })?;

    let Some(fact_type) = FactType::parse(fact_type_raw) else {
        return Err(ReduceError::UnknownFactType {
            fact_type: fact_type_raw.to_string(),
            content: content.clone(),
        });
    };

    if !fact_type.is_view_relevant() {
        return Ok(None);
    }

    let delta = match fact_type {
        FactType::SendMessage => Some(reduce_send_message(params_raw, fact, own_authority)?),
        FactType::CreateChannel => Some(reduce_create_channel(params_raw, fact)?),
        FactType::JoinChannel => Some(reduce_join_channel(params_raw, content)?),
        FactType::LeaveChannel => Some(reduce_leave_channel(params_raw, content)?),
        FactType::CloseChannel => Some(reduce_close_channel(params_raw, content)?),
        FactType::UpdateChannel => reduce_update_channel(params_raw, content)?,
        FactType::SetTopic => Some(reduce_set_topic(params_raw, content)?),
        FactType::SetBlockTopic => Some(reduce_set_block_topic(params_raw, content)?),
        FactType::SetNickname => Some(reduce_set_nickname(params_raw, content)?),
        FactType::SetBlockName => Some(reduce_set_block_name(params_raw, content)?),
        FactType::GrantSteward => Some(reduce_grant_steward(params_raw, content)?),
        FactType::RevokeSteward => Some(reduce_revoke_steward(params_raw, content)?),
        FactType::InitiateRecovery => Some(reduce_initiate_recovery(params_raw, content)?),
        FactType::ApproveRecovery => Some(reduce_approve_recovery(params_raw, content, fact)?),
        FactType::ToggleGuardian => Some(reduce_toggle_guardian(params_raw, content)?),
        FactType::SetGuardianThreshold => Some(reduce_set_guardian_threshold(params_raw, content)?),
        FactType::CreateInvitation => Some(reduce_create_invitation(content)),
        FactType::AcceptInvitation => Some(reduce_accept_invitation(params_raw, content)?),
        FactType::RejectInvitation => Some(reduce_reject_invitation(params_raw, content)?),

        _ => None,
    };

    Ok(delta)
}

fn percent_decode(
    fact_type: &'static str,
    content: &str,
    raw: &str,
) -> Result<String, ReduceError> {
    fn is_hex(b: u8) -> bool {
        matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
    }

    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) => {
                let hi = bytes[i + 1];
                let lo = bytes[i + 2];
                let hex_bytes = [hi, lo];
                let s = std::str::from_utf8(&hex_bytes).expect("hex digits are ASCII");
                let v = u8::from_str_radix(s, 16).expect("hex digits validated");
                out.push(v);
                i += 3;
            }
            b => {
                // Backwards-compatible: treat malformed percent sequences as literal '%'.
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(out).map_err(|e| ReduceError::PercentDecode {
        fact_type,
        reason: format!("invalid utf8: {}", e),
        content: content.to_string(),
    })
}

fn parse_bool(
    fact_type: &'static str,
    content: &str,
    key: &'static str,
    raw: &str,
) -> Result<bool, ReduceError> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ReduceError::InvalidBool {
            fact_type,
            key,
            value: raw.to_string(),
            content: content.to_string(),
        }),
    }
}

fn parse_u32(
    fact_type: &'static str,
    content: &str,
    key: &'static str,
    raw: &str,
) -> Result<u32, ReduceError> {
    raw.parse::<u32>().map_err(|_| ReduceError::InvalidNumber {
        fact_type,
        key,
        value: raw.to_string(),
        content: content.to_string(),
    })
}

fn parse_kv_map(
    fact_type: &'static str,
    content: &str,
    params: &str,
) -> Result<HashMap<String, String>, ReduceError> {
    let mut map = HashMap::new();
    if params.is_empty() {
        return Ok(map);
    }

    for part in params.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type,
                reason: format!("missing '=' in param: {part}"),
                content: content.to_string(),
            })?;

        let key = percent_decode(fact_type, content, k)?;
        let value = percent_decode(fact_type, content, v)?;

        if map.insert(key.clone(), value).is_some() {
            return Err(ReduceError::ParamParse {
                fact_type,
                reason: format!("duplicate param: {key}"),
                content: content.to_string(),
            });
        }
    }

    Ok(map)
}

fn require_param<'a>(
    fact_type: &'static str,
    content: &str,
    map: &'a HashMap<String, String>,
    key: &'static str,
) -> Result<&'a str, ReduceError> {
    map.get(key)
        .map(|s| s.as_str())
        .ok_or_else(|| ReduceError::MissingParam {
            fact_type,
            key,
            content: content.to_string(),
        })
}

fn timestamp_to_ms(timestamp: &TimeStamp) -> u64 {
    match timestamp {
        TimeStamp::PhysicalClock(pt) => pt.ts_ms,
        TimeStamp::LogicalClock(_) => 0,
        TimeStamp::OrderClock(ot) => u64::from_le_bytes(ot.0[..8].try_into().unwrap_or([0u8; 8])),
        TimeStamp::Range(_) => 0,
    }
}

fn reduce_send_message(
    params: &str,
    fact: &JournalFact,
    own_authority: &AuthorityId,
) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SendMessage";

    let (channel_part, rest) =
        params
            .split_once("&content=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&content='".to_string(),
                content: fact.content.clone(),
            })?;

    let channel_id_raw =
        channel_part
            .strip_prefix("channel_id=")
            .ok_or_else(|| ReduceError::MissingParam {
                fact_type: FACT_TYPE,
                key: "channel_id",
                content: fact.content.clone(),
            })?;

    let (content_part, reply_part) =
        rest.split_once("&reply_to=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&reply_to='".to_string(),
                content: fact.content.clone(),
            })?;

    let channel_id_str = percent_decode(FACT_TYPE, &fact.content, channel_id_raw)?;
    let msg_content = percent_decode(FACT_TYPE, &fact.content, content_part)?;
    let reply_to_raw = percent_decode(FACT_TYPE, &fact.content, reply_part)?;
    let reply_to = (reply_to_raw != "None").then_some(reply_to_raw);

    let timestamp = timestamp_to_ms(&fact.timestamp);
    let sender_name = fact.source_authority.to_string();
    let is_own = fact.source_authority == *own_authority;

    let msg_hash = aura_core::hash::hash(fact.content.as_bytes());
    let msg_id = format!(
        "msg_{:x}",
        u64::from_le_bytes(msg_hash[..8].try_into().unwrap_or([0u8; 8]))
    );

    let msg_channel_id = parse_channel_id(&channel_id_str);
    Ok(ViewDelta::MessageSent {
        channel_id: msg_channel_id,
        message: Message {
            id: msg_id,
            channel_id: msg_channel_id,
            sender_id: fact.source_authority,
            sender_name,
            content: msg_content,
            timestamp,
            reply_to,
            is_own,
            is_read: is_own,
        },
    })
}

fn reduce_create_channel(params: &str, fact: &JournalFact) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "CreateChannel";

    let map = parse_kv_map(FACT_TYPE, &fact.content, params)?;

    let name = require_param(FACT_TYPE, &fact.content, &map, "name")?.to_string();
    let topic = map.get("topic").cloned();
    let channel_type_str = require_param(FACT_TYPE, &fact.content, &map, "channel_type")?;

    let channel_type = match channel_type_str {
        "DirectMessage" => ChannelType::DirectMessage,
        "Guardian" => ChannelType::Guardian,
        _ => ChannelType::Block,
    };

    let channel_hash = hash(fact.content.as_bytes());
    let channel_id = ChannelId::from_bytes(channel_hash);

    Ok(ViewDelta::ChannelCreated {
        channel: Channel {
            id: channel_id,
            name,
            topic,
            channel_type,
            unread_count: 0,
            is_dm: channel_type == ChannelType::DirectMessage,
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: timestamp_to_ms(&fact.timestamp),
        },
    })
}

fn reduce_join_channel(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "JoinChannel";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let channel_id_str = require_param(FACT_TYPE, content, &map, "channel_id")?;
    let channel_id = parse_channel_id(channel_id_str);

    Ok(ViewDelta::ChannelJoined { channel_id })
}

fn reduce_leave_channel(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "LeaveChannel";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let channel_id_str = require_param(FACT_TYPE, content, &map, "channel_id")?;
    let channel_id = parse_channel_id(channel_id_str);

    Ok(ViewDelta::ChannelLeft { channel_id })
}

fn reduce_close_channel(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "CloseChannel";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let channel_id_str = require_param(FACT_TYPE, content, &map, "channel_id")?;
    let channel_id = parse_channel_id(channel_id_str);

    Ok(ViewDelta::ChannelClosed { channel_id })
}

fn reduce_update_channel(params: &str, content: &str) -> Result<Option<ViewDelta>, ReduceError> {
    const FACT_TYPE: &str = "UpdateChannel";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let channel_id_str = require_param(FACT_TYPE, content, &map, "channel_id")?;
    let channel_id = parse_channel_id(channel_id_str);

    let description = map.get("description").map(|s| s.as_str()).unwrap_or("");
    if description.trim() == "[closed]" {
        return Ok(Some(ViewDelta::ChannelClosed { channel_id }));
    }

    Ok(None)
}

fn reduce_set_topic(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SetTopic";

    let (channel_part, topic_part) =
        params
            .split_once("&topic=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&topic='".to_string(),
                content: content.to_string(),
            })?;

    let channel_id_raw =
        channel_part
            .strip_prefix("channel_id=")
            .ok_or_else(|| ReduceError::MissingParam {
                fact_type: FACT_TYPE,
                key: "channel_id",
                content: content.to_string(),
            })?;

    let channel_id_str = percent_decode(FACT_TYPE, content, channel_id_raw)?;
    let topic = percent_decode(FACT_TYPE, content, topic_part)?;

    Ok(ViewDelta::TopicUpdated {
        channel_id: parse_channel_id(&channel_id_str),
        topic,
    })
}

fn reduce_set_block_topic(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SetBlockTopic";

    let (block_part, topic_part) =
        params
            .split_once("&topic=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&topic='".to_string(),
                content: content.to_string(),
            })?;

    let block_id_raw =
        block_part
            .strip_prefix("block_id=")
            .ok_or_else(|| ReduceError::MissingParam {
                fact_type: FACT_TYPE,
                key: "block_id",
                content: content.to_string(),
            })?;

    let block_id_str = percent_decode(FACT_TYPE, content, block_id_raw)?;
    let topic = percent_decode(FACT_TYPE, content, topic_part)?;

    Ok(ViewDelta::TopicUpdated {
        channel_id: parse_channel_id(&block_id_str),
        topic,
    })
}

fn reduce_set_nickname(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SetNickname";

    let (target_part, nickname_part) =
        params
            .split_once("&nickname=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&nickname='".to_string(),
                content: content.to_string(),
            })?;

    let target_raw =
        target_part
            .strip_prefix("target=")
            .ok_or_else(|| ReduceError::MissingParam {
                fact_type: FACT_TYPE,
                key: "target",
                content: content.to_string(),
            })?;

    let target_str = percent_decode(FACT_TYPE, content, target_raw)?;
    let nickname = percent_decode(FACT_TYPE, content, nickname_part)?;

    let target = parse_authority_id(FACT_TYPE, content, &target_str)?;
    Ok(ViewDelta::NicknameSet { target, nickname })
}

fn reduce_set_block_name(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SetBlockName";

    let (block_part, name_part) =
        params
            .split_once("&name=")
            .ok_or_else(|| ReduceError::ParamParse {
                fact_type: FACT_TYPE,
                reason: "missing '&name='".to_string(),
                content: content.to_string(),
            })?;

    let block_id_raw =
        block_part
            .strip_prefix("block_id=")
            .ok_or_else(|| ReduceError::MissingParam {
                fact_type: FACT_TYPE,
                key: "block_id",
                content: content.to_string(),
            })?;

    let block_id_str = percent_decode(FACT_TYPE, content, block_id_raw)?;
    let name = percent_decode(FACT_TYPE, content, name_part)?;

    Ok(ViewDelta::BlockNameSet {
        block_id: parse_channel_id(&block_id_str),
        name,
    })
}

fn reduce_grant_steward(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "GrantSteward";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let block_id_str = require_param(FACT_TYPE, content, &map, "block_id")?;
    let target_id_str = require_param(FACT_TYPE, content, &map, "target_id")?;

    let context_id = parse_context_id(FACT_TYPE, content, block_id_str)?;
    let target_id = parse_authority_id(FACT_TYPE, content, target_id_str)?;

    Ok(ViewDelta::StewardGranted {
        context_id,
        target_id,
    })
}

fn reduce_revoke_steward(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "RevokeSteward";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let block_id_str = require_param(FACT_TYPE, content, &map, "block_id")?;
    let target_id_str = require_param(FACT_TYPE, content, &map, "target_id")?;

    let context_id = parse_context_id(FACT_TYPE, content, block_id_str)?;
    let target_id = parse_authority_id(FACT_TYPE, content, target_id_str)?;

    Ok(ViewDelta::StewardRevoked {
        context_id,
        target_id,
    })
}

fn reduce_initiate_recovery(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "InitiateRecovery";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let session_id = require_param(FACT_TYPE, content, &map, "session_id")?.to_string();

    Ok(ViewDelta::RecoveryRequested { session_id })
}

fn reduce_approve_recovery(
    params: &str,
    content: &str,
    fact: &JournalFact,
) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "ApproveRecovery";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let guardian_id_raw = require_param(FACT_TYPE, content, &map, "guardian_id")?;

    let guardian_id = if guardian_id_raw == "self" {
        fact.source_authority
    } else {
        parse_authority_id(FACT_TYPE, content, guardian_id_raw)?
    };

    Ok(ViewDelta::GuardianApproved { guardian_id })
}

fn reduce_toggle_guardian(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "ToggleGuardian";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let contact_id_str = require_param(FACT_TYPE, content, &map, "contact_id")?;
    let is_guardian_raw = require_param(FACT_TYPE, content, &map, "is_guardian")?;

    let contact_id = parse_authority_id(FACT_TYPE, content, contact_id_str)?;
    let is_guardian = parse_bool(FACT_TYPE, content, "is_guardian", is_guardian_raw)?;

    Ok(ViewDelta::GuardianToggled {
        contact_id,
        is_guardian,
    })
}

fn reduce_set_guardian_threshold(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "SetGuardianThreshold";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let threshold_raw = require_param(FACT_TYPE, content, &map, "threshold")?;
    let threshold = parse_u32(FACT_TYPE, content, "threshold", threshold_raw)?;

    Ok(ViewDelta::GuardianThresholdSet { threshold })
}

fn reduce_create_invitation(content: &str) -> ViewDelta {
    let fact_hash = aura_core::hash::hash(content.as_bytes());
    let invitation_id = format!(
        "inv_{:x}",
        u64::from_le_bytes(fact_hash[..8].try_into().unwrap_or([0u8; 8]))
    );

    ViewDelta::InvitationCreated { invitation_id }
}

fn reduce_accept_invitation(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "AcceptInvitation";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let invitation_id = require_param(FACT_TYPE, content, &map, "invitation_id")?.to_string();

    Ok(ViewDelta::InvitationAccepted { invitation_id })
}

fn reduce_reject_invitation(params: &str, content: &str) -> Result<ViewDelta, ReduceError> {
    const FACT_TYPE: &str = "RejectInvitation";

    let map = parse_kv_map(FACT_TYPE, content, params)?;
    let invitation_id = require_param(FACT_TYPE, content, &map, "invitation_id")?.to_string();

    Ok(ViewDelta::InvitationRejected { invitation_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::OrderTime;

    fn make_test_fact(content: &str) -> JournalFact {
        JournalFact {
            content: content.to_string(),
            timestamp: TimeStamp::OrderClock(OrderTime([0u8; 32])),
            source_authority: AuthorityId::new_from_entropy([1u8; 32]),
        }
    }

    #[test]
    fn reduce_send_message_smoke() {
        let fact = make_test_fact("SendMessage::channel_id=ch123&content=Hello&reply_to=None");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority)
            .expect("reduce should succeed")
            .expect("send_message should produce delta");

        let expected_channel_id = parse_channel_id("ch123");

        match delta {
            ViewDelta::MessageSent {
                channel_id,
                message,
            } => {
                assert_eq!(channel_id, expected_channel_id);
                assert_eq!(message.content, "Hello");
                assert!(message.is_own);
            }
            _ => panic!("Expected MessageSent delta"),
        }
    }

    #[test]
    fn reduce_send_message_allows_legacy_ampersand_in_content() {
        let fact =
            make_test_fact("SendMessage::channel_id=ch123&content=Hello&World&reply_to=None");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority)
            .expect("reduce should succeed")
            .expect("send_message should produce delta");

        match delta {
            ViewDelta::MessageSent { message, .. } => {
                assert_eq!(message.content, "Hello&World");
            }
            _ => panic!("Expected MessageSent delta"),
        }
    }

    #[test]
    fn reduce_create_channel_smoke() {
        let fact = make_test_fact("CreateChannel::name=General&topic=Chat&channel_type=Block");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority)
            .expect("reduce should succeed")
            .expect("create_channel should produce delta");

        match delta {
            ViewDelta::ChannelCreated { channel } => {
                assert_eq!(channel.name, "General");
                assert_eq!(channel.topic, Some("Chat".to_string()));
                assert_eq!(channel.channel_type, ChannelType::Block);
            }
            _ => panic!("Expected ChannelCreated delta"),
        }
    }

    #[test]
    fn malformed_view_fact_is_error_not_silent() {
        let fact = make_test_fact("SetNickname::target=not-an-authority&nickname=Bob");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let err = reduce_fact(&fact, &own_authority).expect_err("should error");
        assert!(matches!(err, ReduceError::InvalidId { .. }));
    }

    #[test]
    fn known_non_view_fact_is_ok_none() {
        let fact = make_test_fact("NavigateTo::screen=Chat");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority).expect("reduce should succeed");
        assert!(delta.is_none());
    }

    #[test]
    fn unknown_fact_type_is_error() {
        let fact = make_test_fact("TotallyNewThing::x=1");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let err = reduce_fact(&fact, &own_authority).expect_err("should error");
        assert!(matches!(err, ReduceError::UnknownFactType { .. }));
    }
}

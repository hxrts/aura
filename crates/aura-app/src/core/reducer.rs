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
use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use aura_journal::JournalFact;

/// Result of reducing a fact
#[derive(Debug, Clone)]
pub enum ViewDelta {
    /// A new message was sent
    MessageSent {
        channel_id: String,
        message: Message,
    },
    /// A channel was created
    ChannelCreated { channel: Channel },
    /// A channel was joined
    ChannelJoined { channel_id: String },
    /// A channel was left
    ChannelLeft { channel_id: String },
    /// A channel was closed/archived
    ChannelClosed { channel_id: String },
    /// Channel topic was updated
    TopicUpdated { channel_id: String, topic: String },
    /// A petname was set for a contact
    PetnameSet { target: String, petname: String },
    /// A block name was set
    BlockNameSet { block_id: String, name: String },
    /// A recovery request was initiated
    RecoveryRequested { session_id: String },
    /// A guardian approval was granted
    GuardianApproved { guardian_id: String },
    /// Guardian status was toggled for a contact
    GuardianToggled {
        contact_id: String,
        is_guardian: bool,
    },
    /// Guardian threshold was configured
    GuardianThresholdSet { threshold: u32 },
    /// An invitation was created
    InvitationCreated { invitation_id: String },
    /// An invitation was accepted
    InvitationAccepted { invitation_id: String },
    /// An invitation was rejected
    InvitationRejected { invitation_id: String },
    /// Unknown or unhandled fact type
    Unknown { content: String },
}

/// Reduce a journal fact into a view delta
pub fn reduce_fact(fact: &JournalFact, own_authority: &AuthorityId) -> ViewDelta {
    let content = &fact.content;

    // Parse the fact content based on its prefix
    if content.starts_with("SendMessage::") {
        reduce_send_message(content, fact, own_authority)
    } else if content.starts_with("CreateChannel::") {
        reduce_create_channel(content, fact)
    } else if content.starts_with("JoinChannel::") {
        reduce_join_channel(content)
    } else if content.starts_with("LeaveChannel::") {
        reduce_leave_channel(content)
    } else if content.starts_with("CloseChannel::") {
        reduce_close_channel(content)
    } else if content.starts_with("SetTopic::") {
        reduce_set_topic(content)
    } else if content.starts_with("SetPetname::") {
        reduce_set_petname(content)
    } else if content.starts_with("SetBlockName::") {
        reduce_set_block_name(content)
    } else if content.starts_with("InitiateRecovery::") {
        reduce_initiate_recovery(content)
    } else if content.starts_with("ApproveRecovery::") {
        reduce_approve_recovery(content)
    } else if content.starts_with("ToggleGuardian::") {
        reduce_toggle_guardian(content)
    } else if content.starts_with("SetGuardianThreshold::") {
        reduce_set_guardian_threshold(content)
    } else if content.starts_with("CreateInvitation::") {
        reduce_create_invitation(content)
    } else if content.starts_with("AcceptInvitation::") {
        reduce_accept_invitation(content)
    } else if content.starts_with("RejectInvitation::") {
        reduce_reject_invitation(content)
    } else {
        ViewDelta::Unknown {
            content: content.clone(),
        }
    }
}

/// Parse a key=value parameter from content
fn parse_param<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    content.split('&').find_map(|part| {
        if part.starts_with(&prefix) {
            Some(&part[prefix.len()..])
        } else {
            None
        }
    })
}

/// Extract timestamp as milliseconds from epoch
fn timestamp_to_ms(timestamp: &TimeStamp) -> u64 {
    match timestamp {
        TimeStamp::PhysicalClock(pt) => pt.ts_ms,
        TimeStamp::LogicalClock(_) => 0, // Logical clocks don't have wall time
        TimeStamp::OrderClock(ot) => {
            // Use first 8 bytes as a pseudo-timestamp for ordering
            u64::from_le_bytes(ot.0[..8].try_into().unwrap_or([0u8; 8]))
        }
        TimeStamp::Range(_) => 0, // Ranges don't have a single point
    }
}

fn reduce_send_message(
    content: &str,
    fact: &JournalFact,
    own_authority: &AuthorityId,
) -> ViewDelta {
    // Parse: SendMessage::channel_id={...}&content={...}&reply_to={...}
    let params = content.strip_prefix("SendMessage::").unwrap_or(content);
    let channel_id = parse_param(params, "channel_id")
        .unwrap_or("unknown")
        .to_string();
    let msg_content = parse_param(params, "content").unwrap_or("").to_string();
    let reply_to = parse_param(params, "reply_to")
        .filter(|v| *v != "None")
        .map(|s| s.to_string());

    let timestamp = timestamp_to_ms(&fact.timestamp);
    let sender_id = fact.source_authority.to_string();
    let is_own = fact.source_authority == *own_authority;

    // Generate a message ID from the fact content hash
    let msg_hash = aura_core::hash::hash(content.as_bytes());
    let msg_id = format!(
        "msg_{:x}",
        u64::from_le_bytes(msg_hash[..8].try_into().unwrap_or([0u8; 8]))
    );

    let msg_channel_id = channel_id.clone();
    ViewDelta::MessageSent {
        channel_id,
        message: Message {
            id: msg_id,
            channel_id: msg_channel_id,
            sender_id: sender_id.clone(),
            sender_name: sender_id, // Petname resolved in ViewState::apply_delta
            content: msg_content,
            timestamp,
            reply_to,
            is_own,
            is_read: is_own, // Own messages are automatically read
        },
    }
}

fn reduce_create_channel(content: &str, fact: &JournalFact) -> ViewDelta {
    let params = content.strip_prefix("CreateChannel::").unwrap_or(content);
    let name = parse_param(params, "name")
        .unwrap_or("New Channel")
        .to_string();
    let topic = parse_param(params, "topic").map(|s| s.to_string());
    let channel_type_str = parse_param(params, "channel_type").unwrap_or("Block");

    let channel_type = match channel_type_str {
        "DirectMessage" => ChannelType::DirectMessage,
        "Guardian" => ChannelType::Guardian,
        _ => ChannelType::Block,
    };

    // Generate channel ID from content hash
    let channel_hash = aura_core::hash::hash(content.as_bytes());
    let channel_id = format!(
        "ch_{:x}",
        u64::from_le_bytes(channel_hash[..8].try_into().unwrap_or([0u8; 8]))
    );

    ViewDelta::ChannelCreated {
        channel: Channel {
            id: channel_id,
            name,
            topic,
            channel_type,
            unread_count: 0,
            is_dm: channel_type == ChannelType::DirectMessage,
            member_count: 1, // Creator is first member
            last_message: None,
            last_message_time: None,
            last_activity: timestamp_to_ms(&fact.timestamp),
        },
    }
}

fn reduce_join_channel(content: &str) -> ViewDelta {
    let params = content.strip_prefix("JoinChannel::").unwrap_or(content);
    let channel_id = parse_param(params, "channel_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::ChannelJoined { channel_id }
}

fn reduce_leave_channel(content: &str) -> ViewDelta {
    let params = content.strip_prefix("LeaveChannel::").unwrap_or(content);
    let channel_id = parse_param(params, "channel_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::ChannelLeft { channel_id }
}

fn reduce_close_channel(content: &str) -> ViewDelta {
    let params = content.strip_prefix("CloseChannel::").unwrap_or(content);
    let channel_id = parse_param(params, "channel_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::ChannelClosed { channel_id }
}

fn reduce_set_topic(content: &str) -> ViewDelta {
    let params = content.strip_prefix("SetTopic::").unwrap_or(content);
    let channel_id = parse_param(params, "channel_id")
        .unwrap_or("unknown")
        .to_string();
    let topic = parse_param(params, "topic").unwrap_or("").to_string();

    ViewDelta::TopicUpdated { channel_id, topic }
}

fn reduce_set_petname(content: &str) -> ViewDelta {
    let params = content.strip_prefix("SetPetname::").unwrap_or(content);
    let target = parse_param(params, "target")
        .unwrap_or("unknown")
        .to_string();
    let petname = parse_param(params, "petname").unwrap_or("").to_string();

    ViewDelta::PetnameSet { target, petname }
}

fn reduce_set_block_name(content: &str) -> ViewDelta {
    let params = content.strip_prefix("SetBlockName::").unwrap_or(content);
    let block_id = parse_param(params, "block_id")
        .unwrap_or("unknown")
        .to_string();
    let name = parse_param(params, "name").unwrap_or("").to_string();

    ViewDelta::BlockNameSet { block_id, name }
}

fn reduce_initiate_recovery(content: &str) -> ViewDelta {
    let params = content
        .strip_prefix("InitiateRecovery::")
        .unwrap_or(content);
    let session_id = parse_param(params, "session_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::RecoveryRequested { session_id }
}

fn reduce_approve_recovery(content: &str) -> ViewDelta {
    let params = content.strip_prefix("ApproveRecovery::").unwrap_or(content);
    let guardian_id = parse_param(params, "guardian_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::GuardianApproved { guardian_id }
}

fn reduce_toggle_guardian(content: &str) -> ViewDelta {
    let params = content.strip_prefix("ToggleGuardian::").unwrap_or(content);
    let contact_id = parse_param(params, "contact_id")
        .unwrap_or("unknown")
        .to_string();
    let is_guardian = parse_param(params, "is_guardian")
        .map(|s| s == "true")
        .unwrap_or(false);

    ViewDelta::GuardianToggled {
        contact_id,
        is_guardian,
    }
}

fn reduce_set_guardian_threshold(content: &str) -> ViewDelta {
    let params = content
        .strip_prefix("SetGuardianThreshold::")
        .unwrap_or(content);
    let threshold = parse_param(params, "threshold")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

    ViewDelta::GuardianThresholdSet { threshold }
}

fn reduce_create_invitation(content: &str) -> ViewDelta {
    let params = content
        .strip_prefix("CreateInvitation::")
        .unwrap_or(content);
    let invitation_id = parse_param(params, "invitation_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::InvitationCreated { invitation_id }
}

fn reduce_accept_invitation(content: &str) -> ViewDelta {
    let params = content
        .strip_prefix("AcceptInvitation::")
        .unwrap_or(content);
    let invitation_id = parse_param(params, "invitation_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::InvitationAccepted { invitation_id }
}

fn reduce_reject_invitation(content: &str) -> ViewDelta {
    let params = content
        .strip_prefix("RejectInvitation::")
        .unwrap_or(content);
    let invitation_id = parse_param(params, "invitation_id")
        .unwrap_or("unknown")
        .to_string();

    ViewDelta::InvitationRejected { invitation_id }
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
    fn test_reduce_send_message() {
        let fact = make_test_fact("SendMessage::channel_id=ch123&content=Hello&reply_to=None");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority);

        match delta {
            ViewDelta::MessageSent {
                channel_id,
                message,
            } => {
                assert_eq!(channel_id, "ch123");
                assert_eq!(message.content, "Hello");
                assert!(message.is_own);
            }
            _ => panic!("Expected MessageSent delta"),
        }
    }

    #[test]
    fn test_reduce_create_channel() {
        let fact = make_test_fact("CreateChannel::name=General&topic=Chat&channel_type=Block");
        let own_authority = AuthorityId::new_from_entropy([1u8; 32]);
        let delta = reduce_fact(&fact, &own_authority);

        match delta {
            ViewDelta::ChannelCreated { channel } => {
                assert_eq!(channel.name, "General");
                assert_eq!(channel.topic, Some("Chat".to_string()));
                assert_eq!(channel.channel_type, ChannelType::Block);
            }
            _ => panic!("Expected ChannelCreated delta"),
        }
    }
}

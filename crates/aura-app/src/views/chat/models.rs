#![allow(missing_docs)]

use super::delivery::MessageDeliveryStatus;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};

/// Type of channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ChannelType {
    #[default]
    Home,
    DirectMessage,
    Guardian,
    All,
}

impl ChannelType {
    /// Alias for Home (group channels).
    pub const GROUP: ChannelType = ChannelType::Home;
}

/// A chat channel.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Channel {
    pub id: ChannelId,
    #[serde(default)]
    pub context_id: Option<ContextId>,
    pub name: String,
    pub topic: Option<String>,
    pub channel_type: ChannelType,
    pub unread_count: u32,
    pub is_dm: bool,
    #[serde(default)]
    pub member_ids: Vec<AuthorityId>,
    pub member_count: u32,
    pub last_message: Option<String>,
    pub last_message_time: Option<u64>,
    pub last_activity: u64,
    #[serde(default)]
    pub last_finalized_epoch: u32,
}

/// A chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Message {
    pub id: String,
    pub channel_id: ChannelId,
    pub sender_id: AuthorityId,
    pub sender_name: String,
    pub content: String,
    pub timestamp: u64,
    pub reply_to: Option<String>,
    pub is_own: bool,
    pub is_read: bool,
    #[serde(default)]
    pub delivery_status: MessageDeliveryStatus,
    #[serde(default)]
    pub epoch_hint: Option<u32>,
    #[serde(default)]
    pub is_finalized: bool,
}

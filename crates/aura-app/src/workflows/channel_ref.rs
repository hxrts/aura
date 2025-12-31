//! Shared channel reference helpers for workflows.

use aura_core::crypto::hash::hash;
use aura_core::identifiers::ChannelId;

/// Reference to a channel identifier or name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub enum ChannelRef {
    /// Canonical channel id.
    Id(ChannelId),
    /// Human-friendly name (hashed deterministically).
    Name(String),
}

impl ChannelRef {
    #[cfg_attr(not(feature = "signals"), allow(dead_code))]
    pub fn parse(input: &str) -> Self {
        let normalized = normalize_channel_str(input);
        match normalized.parse::<ChannelId>() {
            Ok(id) => ChannelRef::Id(id),
            Err(_) => ChannelRef::Name(normalized.to_string()),
        }
    }

    #[cfg_attr(not(feature = "signals"), allow(dead_code))]
    pub fn to_channel_id(&self) -> ChannelId {
        match self {
            ChannelRef::Id(id) => *id,
            ChannelRef::Name(name) => ChannelId::from_bytes(hash(name.to_lowercase().as_bytes())),
        }
    }
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
fn normalize_channel_str(channel: &str) -> &str {
    channel.strip_prefix("home:").unwrap_or(channel)
}

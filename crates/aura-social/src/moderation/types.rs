//! Types for moderation query results

use aura_core::identifiers::{AuthorityId, ChannelId};
use serde::{Deserialize, Serialize};

/// Current ban status for a user
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanStatus {
    /// Authority ID of the banned user
    pub banned_authority: AuthorityId,
    /// Authority ID of the steward who issued the ban
    pub actor_authority: AuthorityId,
    /// Reason for the ban
    pub reason: String,
    /// Timestamp when ban was issued (ms since epoch)
    pub banned_at_ms: u64,
    /// Optional expiration timestamp (ms since epoch)
    pub expires_at_ms: Option<u64>,
    /// Optional channel-specific ban (None = block-wide)
    pub channel_id: Option<ChannelId>,
}

/// Current mute status for a user
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MuteStatus {
    /// Authority ID of the muted user
    pub muted_authority: AuthorityId,
    /// Authority ID of the steward who issued the mute
    pub actor_authority: AuthorityId,
    /// Duration in seconds (if specified)
    pub duration_secs: Option<u64>,
    /// Timestamp when mute was issued (ms since epoch)
    pub muted_at_ms: u64,
    /// Optional expiration timestamp (ms since epoch)
    pub expires_at_ms: Option<u64>,
    /// Optional channel-specific mute (None = block-wide)
    pub channel_id: Option<ChannelId>,
}

/// Kick record from audit log
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KickRecord {
    /// Authority ID of the kicked user
    pub kicked_authority: AuthorityId,
    /// Authority ID of the steward who issued the kick
    pub actor_authority: AuthorityId,
    /// Channel where kick occurred
    pub channel_id: ChannelId,
    /// Reason for the kick
    pub reason: String,
    /// Timestamp when kick occurred (ms since epoch)
    pub kicked_at_ms: u64,
}

impl BanStatus {
    /// Check if this ban is expired at the given timestamp
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        self.expires_at_ms
            .map(|exp| current_time_ms >= exp)
            .unwrap_or(false)
    }

    /// Check if this ban applies to a specific channel
    pub fn applies_to_channel(&self, channel: &ChannelId) -> bool {
        self.channel_id
            .as_ref()
            .map(|ch| ch == channel)
            .unwrap_or(true) // Block-wide bans apply to all channels
    }
}

impl MuteStatus {
    /// Check if this mute is expired at the given timestamp
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        self.expires_at_ms
            .map(|exp| current_time_ms >= exp)
            .unwrap_or(false)
    }

    /// Check if this mute applies to a specific channel
    pub fn applies_to_channel(&self, channel: &ChannelId) -> bool {
        self.channel_id
            .as_ref()
            .map(|ch| ch == channel)
            .unwrap_or(true) // Block-wide mutes apply to all channels
    }
}

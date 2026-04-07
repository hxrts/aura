#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ChannelId};
use serde::{Deserialize, Serialize};

/// Ban record for persistent moderation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BanRecord {
    pub authority_id: AuthorityId,
    pub reason: String,
    pub actor: AuthorityId,
    pub banned_at: u64,
}

/// Mute record for persistent moderation with expiration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MuteRecord {
    pub authority_id: AuthorityId,
    pub duration_secs: Option<u64>,
    pub muted_at: u64,
    pub expires_at: Option<u64>,
    pub actor: AuthorityId,
}

impl MuteRecord {
    /// Check if this mute has expired.
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        match self.expires_at {
            Some(expiry) => current_time_ms >= expiry,
            None => false,
        }
    }
}

/// Kick log entry for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct KickRecord {
    pub authority_id: AuthorityId,
    pub channel: ChannelId,
    pub reason: String,
    pub actor: AuthorityId,
    pub kicked_at: u64,
}

/// Pinned message metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PinnedMessageMeta {
    pub message_id: String,
    pub pinned_by: AuthorityId,
    pub pinned_at: u64,
}

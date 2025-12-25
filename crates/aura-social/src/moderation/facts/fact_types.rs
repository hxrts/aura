//! Moderation fact type definitions

use super::constants::*;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::PhysicalTime;
use aura_journal::DomainFact;
use serde::{Deserialize, Serialize};

fn serialize_fact<T: Serialize>(value: &T, label: &'static str) -> Vec<u8> {
    let bytes = serde_json::to_vec(value);
    debug_assert!(bytes.is_ok(), "failed to serialize {label}");
    bytes.unwrap_or_default()
}

/// Fact representing a block-wide mute or channel-specific mute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMuteFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub muted_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub duration_secs: Option<u64>,
    pub muted_at: PhysicalTime,
    pub expires_at: Option<PhysicalTime>,
}

impl BlockMuteFact {
    /// Backward-compat accessor for muted_at timestamp in milliseconds.
    pub fn muted_at_ms(&self) -> u64 {
        self.muted_at.ts_ms
    }

    /// Backward-compat accessor for expires_at timestamp in milliseconds.
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at.as_ref().map(|t| t.ts_ms)
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        muted_authority: AuthorityId,
        actor_authority: AuthorityId,
        duration_secs: Option<u64>,
        muted_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            muted_authority,
            actor_authority,
            duration_secs,
            muted_at: PhysicalTime {
                ts_ms: muted_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
        }
    }
}

impl DomainFact for BlockMuteFact {
    fn type_id(&self) -> &'static str {
        BLOCK_MUTE_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block mute fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing the removal of a block mute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUnmuteFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub unmuted_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub unmuted_at: PhysicalTime,
}

impl BlockUnmuteFact {
    /// Backward-compat accessor for unmuted_at timestamp in milliseconds.
    pub fn unmuted_at_ms(&self) -> u64 {
        self.unmuted_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        unmuted_authority: AuthorityId,
        actor_authority: AuthorityId,
        unmuted_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            unmuted_authority,
            actor_authority,
            unmuted_at: PhysicalTime {
                ts_ms: unmuted_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockUnmuteFact {
    fn type_id(&self) -> &'static str {
        BLOCK_UNMUTE_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block unmute fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing a block-wide ban or channel-specific ban.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBanFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub banned_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub reason: String,
    pub banned_at: PhysicalTime,
    pub expires_at: Option<PhysicalTime>,
}

impl BlockBanFact {
    /// Backward-compat accessor for banned_at timestamp in milliseconds.
    pub fn banned_at_ms(&self) -> u64 {
        self.banned_at.ts_ms
    }

    /// Backward-compat accessor for expires_at timestamp in milliseconds.
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at.as_ref().map(|t| t.ts_ms)
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        banned_authority: AuthorityId,
        actor_authority: AuthorityId,
        reason: String,
        banned_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            banned_authority,
            actor_authority,
            reason,
            banned_at: PhysicalTime {
                ts_ms: banned_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
        }
    }
}

impl DomainFact for BlockBanFact {
    fn type_id(&self) -> &'static str {
        BLOCK_BAN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block ban fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing the removal of a block ban.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUnbanFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub unbanned_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub unbanned_at: PhysicalTime,
}

impl BlockUnbanFact {
    /// Backward-compat accessor for unbanned_at timestamp in milliseconds.
    pub fn unbanned_at_ms(&self) -> u64 {
        self.unbanned_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        unbanned_authority: AuthorityId,
        actor_authority: AuthorityId,
        unbanned_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            unbanned_authority,
            actor_authority,
            unbanned_at: PhysicalTime {
                ts_ms: unbanned_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockUnbanFact {
    fn type_id(&self) -> &'static str {
        BLOCK_UNBAN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block unban fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing a kick from a channel (audit log entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockKickFact {
    pub context_id: ContextId,
    pub channel_id: ChannelId,
    pub kicked_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub reason: String,
    pub kicked_at: PhysicalTime,
}

impl BlockKickFact {
    /// Backward-compat accessor for kicked_at timestamp in milliseconds.
    pub fn kicked_at_ms(&self) -> u64 {
        self.kicked_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        kicked_authority: AuthorityId,
        actor_authority: AuthorityId,
        reason: String,
        kicked_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            kicked_authority,
            actor_authority,
            reason,
            kicked_at: PhysicalTime {
                ts_ms: kicked_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockKickFact {
    fn type_id(&self) -> &'static str {
        BLOCK_KICK_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block kick fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing a pinned message in a block/channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPinFact {
    pub context_id: ContextId,
    pub channel_id: ChannelId,
    pub message_id: String,
    pub actor_authority: AuthorityId,
    pub pinned_at: PhysicalTime,
}

impl BlockPinFact {
    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        actor_authority: AuthorityId,
        pinned_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            message_id,
            actor_authority,
            pinned_at: PhysicalTime {
                ts_ms: pinned_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockPinFact {
    fn type_id(&self) -> &'static str {
        BLOCK_PIN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block pin fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing an unpinned message in a block/channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUnpinFact {
    pub context_id: ContextId,
    pub channel_id: ChannelId,
    pub message_id: String,
    pub actor_authority: AuthorityId,
    pub unpinned_at: PhysicalTime,
}

impl BlockUnpinFact {
    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        actor_authority: AuthorityId,
        unpinned_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            message_id,
            actor_authority,
            unpinned_at: PhysicalTime {
                ts_ms: unpinned_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockUnpinFact {
    fn type_id(&self) -> &'static str {
        BLOCK_UNPIN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block unpin fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing granting steward (admin) privileges to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockGrantStewardFact {
    /// Block context where steward is being granted
    pub context_id: ContextId,
    /// Authority being granted steward status
    pub target_authority: AuthorityId,
    /// Authority performing the grant (must be existing steward or owner)
    pub actor_authority: AuthorityId,
    /// When steward was granted
    pub granted_at: PhysicalTime,
}

impl BlockGrantStewardFact {
    /// Accessor for granted_at timestamp in milliseconds.
    pub fn granted_at_ms(&self) -> u64 {
        self.granted_at.ts_ms
    }

    /// Constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        target_authority: AuthorityId,
        actor_authority: AuthorityId,
        granted_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            target_authority,
            actor_authority,
            granted_at: PhysicalTime {
                ts_ms: granted_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockGrantStewardFact {
    fn type_id(&self) -> &'static str {
        BLOCK_GRANT_STEWARD_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block grant steward fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing revoking steward (admin) privileges from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRevokeStewardFact {
    /// Block context where steward is being revoked
    pub context_id: ContextId,
    /// Authority having steward status revoked
    pub target_authority: AuthorityId,
    /// Authority performing the revocation (must be existing steward or owner)
    pub actor_authority: AuthorityId,
    /// When steward was revoked
    pub revoked_at: PhysicalTime,
}

impl BlockRevokeStewardFact {
    /// Accessor for revoked_at timestamp in milliseconds.
    pub fn revoked_at_ms(&self) -> u64 {
        self.revoked_at.ts_ms
    }

    /// Constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        target_authority: AuthorityId,
        actor_authority: AuthorityId,
        revoked_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            target_authority,
            actor_authority,
            revoked_at: PhysicalTime {
                ts_ms: revoked_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockRevokeStewardFact {
    fn type_id(&self) -> &'static str {
        BLOCK_REVOKE_STEWARD_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serialize_fact(self, "block revoke steward fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

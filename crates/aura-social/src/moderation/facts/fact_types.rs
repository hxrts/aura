//! Moderation fact type definitions

use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::PhysicalTime;
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Fact representing a block-wide mute or channel-specific mute.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-mute",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockMuteFact {
    /// Context where the mute applies
    pub context_id: ContextId,
    /// Optional channel restriction (None = block-wide)
    pub channel_id: Option<ChannelId>,
    /// Authority being muted
    pub muted_authority: AuthorityId,
    /// Authority who performed the mute
    pub actor_authority: AuthorityId,
    /// Optional duration in seconds (None = permanent)
    pub duration_secs: Option<u64>,
    /// When the mute was applied
    pub muted_at: PhysicalTime,
    /// When the mute expires (None = permanent)
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

/// Fact representing the removal of a block mute.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-unmute",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockUnmuteFact {
    /// Context where the unmute applies
    pub context_id: ContextId,
    /// Optional channel restriction (None = block-wide)
    pub channel_id: Option<ChannelId>,
    /// Authority being unmuted
    pub unmuted_authority: AuthorityId,
    /// Authority who performed the unmute
    pub actor_authority: AuthorityId,
    /// When the unmute was applied
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

/// Fact representing a block-wide ban or channel-specific ban.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-ban",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockBanFact {
    /// Context where the ban applies
    pub context_id: ContextId,
    /// Optional channel restriction (None = block-wide)
    pub channel_id: Option<ChannelId>,
    /// Authority being banned
    pub banned_authority: AuthorityId,
    /// Authority who performed the ban
    pub actor_authority: AuthorityId,
    /// Reason for the ban
    pub reason: String,
    /// When the ban was applied
    pub banned_at: PhysicalTime,
    /// When the ban expires (None = permanent)
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

/// Fact representing the removal of a block ban.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-unban",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockUnbanFact {
    /// Context where the unban applies
    pub context_id: ContextId,
    /// Optional channel restriction (None = block-wide)
    pub channel_id: Option<ChannelId>,
    /// Authority being unbanned
    pub unbanned_authority: AuthorityId,
    /// Authority who performed the unban
    pub actor_authority: AuthorityId,
    /// When the unban was applied
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

/// Fact representing a kick from a channel (audit log entry).
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-kick",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockKickFact {
    /// Context where the kick occurred
    pub context_id: ContextId,
    /// Channel from which the user was kicked
    pub channel_id: ChannelId,
    /// Authority being kicked
    pub kicked_authority: AuthorityId,
    /// Authority who performed the kick
    pub actor_authority: AuthorityId,
    /// Reason for the kick
    pub reason: String,
    /// When the kick occurred
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

/// Fact representing a pinned message in a block/channel.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-pin",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockPinFact {
    /// Context where the pin applies
    pub context_id: ContextId,
    /// Channel containing the pinned message
    pub channel_id: ChannelId,
    /// ID of the pinned message
    pub message_id: String,
    /// Authority who pinned the message
    pub actor_authority: AuthorityId,
    /// When the message was pinned
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

/// Fact representing an unpinned message in a block/channel.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-unpin",
    schema_version = 1,
    context = "context_id"
)]
pub struct BlockUnpinFact {
    /// Context where the unpin applies
    pub context_id: ContextId,
    /// Channel containing the unpinned message
    pub channel_id: ChannelId,
    /// ID of the unpinned message
    pub message_id: String,
    /// Authority who unpinned the message
    pub actor_authority: AuthorityId,
    /// When the message was unpinned
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

/// Fact representing granting steward (admin) privileges to a user.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-grant-steward",
    schema_version = 1,
    context = "context_id"
)]
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

/// Fact representing revoking steward (admin) privileges from a user.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "moderation:block-revoke-steward",
    schema_version = 1,
    context = "context_id"
)]
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

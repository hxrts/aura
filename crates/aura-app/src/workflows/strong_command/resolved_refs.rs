#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use std::fmt;

/// Canonical authority target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedAuthorityId(pub AuthorityId);

/// Canonical channel target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedChannelId(pub ChannelId);

/// Canonical context target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedContextId(pub ContextId);

/// Canonical existing channel target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistingChannelResolution {
    channel_id: ResolvedChannelId,
    context_id: Option<ResolvedContextId>,
}

impl ExistingChannelResolution {
    #[must_use]
    pub(crate) const fn new(
        channel_id: ResolvedChannelId,
        context_id: Option<ResolvedContextId>,
    ) -> Self {
        Self {
            channel_id,
            context_id,
        }
    }

    #[must_use]
    pub const fn channel_id(&self) -> ResolvedChannelId {
        self.channel_id
    }

    #[must_use]
    pub const fn context_id(&self) -> Option<ResolvedContextId> {
        self.context_id
    }
}

/// Canonical result of channel resolution for commands that may target an
/// existing channel or intentionally create one later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelResolveOutcome {
    Existing(ExistingChannelResolution),
    WillCreate { channel_name: String },
}

impl ChannelResolveOutcome {
    #[must_use]
    pub const fn context_id(&self) -> Option<ResolvedContextId> {
        match self {
            Self::Existing(channel) => channel.context_id(),
            Self::WillCreate { .. } => None,
        }
    }

    #[must_use]
    pub const fn existing_channel(&self) -> Option<ExistingChannelResolution> {
        match self {
            Self::Existing(channel) => Some(*channel),
            Self::WillCreate { .. } => None,
        }
    }

    #[must_use]
    pub fn is_will_create(&self) -> bool {
        matches!(self, Self::WillCreate { .. })
    }
}

/// Executable command values with canonical IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCommand {
    Msg {
        target: ResolvedAuthorityId,
        text: String,
    },
    Me {
        action: String,
    },
    Nick {
        name: String,
    },
    Who,
    Whois {
        target: ResolvedAuthorityId,
    },
    Leave,
    Join {
        channel_name: String,
        channel: ChannelResolveOutcome,
    },
    Help {
        command: Option<String>,
    },
    Neighborhood {
        name: String,
    },
    NhAdd {
        home_id: String,
    },
    NhLink {
        home_id: String,
    },
    HomeInvite {
        target: ResolvedAuthorityId,
    },
    HomeAccept,
    Kick {
        target: ResolvedAuthorityId,
        reason: Option<String>,
    },
    Ban {
        target: ResolvedAuthorityId,
        reason: Option<String>,
    },
    Unban {
        target: ResolvedAuthorityId,
    },
    Mute {
        target: ResolvedAuthorityId,
        duration: Option<std::time::Duration>,
    },
    Unmute {
        target: ResolvedAuthorityId,
    },
    Invite {
        target: ResolvedAuthorityId,
    },
    Topic {
        text: String,
    },
    Pin {
        message_id: String,
    },
    Unpin {
        message_id: String,
    },
    Op {
        target: ResolvedAuthorityId,
    },
    Deop {
        target: ResolvedAuthorityId,
    },
    Mode {
        channel_name: String,
        channel: ExistingChannelResolution,
        flags: String,
    },
}

/// Resolution target namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveTarget {
    Authority,
    Channel,
    Context,
}

impl fmt::Display for ResolveTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authority => write!(f, "authority"),
            Self::Channel => write!(f, "channel"),
            Self::Context => write!(f, "context"),
        }
    }
}

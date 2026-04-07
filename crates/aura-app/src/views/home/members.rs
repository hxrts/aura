#![allow(missing_docs)]

use aura_core::types::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

/// Role designation in a home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum HomeRole {
    #[default]
    Participant,
    Member,
    Moderator,
}

impl HomeRole {
    /// Returns true when the role participates in the threshold authority.
    pub const fn is_threshold_member(self) -> bool {
        matches!(self, Self::Member | Self::Moderator)
    }

    /// Returns true for non-threshold participants.
    pub const fn is_participant(self) -> bool {
        matches!(self, Self::Participant)
    }
}

/// A home member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomeMember {
    pub id: AuthorityId,
    pub name: String,
    pub role: HomeRole,
    pub is_online: bool,
    pub joined_at: u64,
    pub last_seen: Option<u64>,
    pub storage_allocated: u64,
}

impl HomeMember {
    /// Check if this member has moderator designation.
    pub fn is_moderator(&self) -> bool {
        matches!(self.role, HomeRole::Moderator)
    }

    /// Check if this member participates in threshold authority membership.
    pub fn is_threshold_member(&self) -> bool {
        self.role.is_threshold_member()
    }
}
